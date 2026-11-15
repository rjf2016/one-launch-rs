mod attribution;
mod dependencies;
mod event;
mod evidence;
mod git;
mod paths;
mod rows;
mod ticket;
mod toml;

#[cfg(test)]
#[path = "../../tests/policy/mod.rs"]
mod tests;

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};

use crate::USAGE;
use event::{Event, PullRequest, Review};
use git::{Change, Commit, Entry, Repo, Tree, short};
use ticket::Ticket;

const DEFAULT_BASE: &str = "refs/remotes/origin/main";
const NO_TICKET_NEEDED: &str = "no ticket: every changed path is under plan/, so none is needed and rules 2 and 7 have nothing to judge";
const NOT_JUDGED: &str = "not judged: there is no ticket to judge by";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    Ticket,
    Scope,
    Immutable,
    Apart,
    OwnersRows,
    Attribution,
    Dependencies,
    Evidence,
}

impl Rule {
    fn name(self) -> &'static str {
        match self {
            Self::Ticket => "ticket",
            Self::Scope => "scope",
            Self::Immutable => "tickets are immutable",
            Self::Apart => "plan and code apart",
            Self::OwnersRows => "the owner's rows",
            Self::Attribution => "attribution",
            Self::Dependencies => "dependencies",
            Self::Evidence => "evidence",
        }
    }
}

struct Verdict {
    rule: Rule,
    problems: Vec<String>,
    notes: Vec<String>,
}

impl Verdict {
    fn of(rule: Rule) -> Self {
        Self {
            rule,
            problems: Vec::new(),
            notes: Vec::new(),
        }
    }
}

pub struct Report {
    about: String,
    verdicts: Vec<Verdict>,
}

impl Report {
    fn about(about: String) -> Self {
        Self {
            about,
            verdicts: Vec::new(),
        }
    }

    fn add(&mut self, rule: Rule, problems: Vec<String>) {
        self.verdicts.push(Verdict {
            problems,
            ..Verdict::of(rule)
        });
    }

    fn note(&mut self, rule: Rule, note: &str) {
        self.verdicts.push(Verdict {
            notes: vec![note.to_owned()],
            ..Verdict::of(rule)
        });
    }

    fn failed(&self) -> Vec<Rule> {
        self.verdicts
            .iter()
            .filter(|verdict| !verdict.problems.is_empty())
            .map(|verdict| verdict.rule)
            .collect()
    }

    fn print(&self, what: &str) -> Result<()> {
        println!("{what}: {}", self.about);
        for verdict in &self.verdicts {
            println!("== {}", verdict.rule.name());
            for line in verdict.notes.iter().chain(&verdict.problems) {
                println!("{line}");
            }
        }
        let failed: Vec<&str> = self.failed().into_iter().map(Rule::name).collect();
        if !failed.is_empty() {
            bail!("{what} failed: {}", failed.join(", "));
        }
        println!("== {what} passed");
        Ok(())
    }
}

pub fn run(args: &[String]) -> Result<()> {
    on_event(&Repo::workspace(), args)?.print("policy")
}

pub fn check(args: &[String]) -> Result<()> {
    on_checkout(&Repo::workspace(), args)?.print("check --ticket")
}

fn options<'a>(args: &'a [String], known: &[&str]) -> Result<BTreeMap<&'a str, &'a str>> {
    let mut options = BTreeMap::new();
    for pair in args.chunks(2) {
        let [name, value] = pair else {
            bail!(USAGE);
        };
        let new = options.insert(name.as_str(), value.as_str()).is_none();
        if !new || !known.contains(&name.as_str()) {
            bail!(USAGE);
        }
    }
    Ok(options)
}

fn file(path: &str) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("could not read {path}"))
}

fn on_event(repo: &Repo, args: &[String]) -> Result<Report> {
    let options = options(args, &["--base", "--event", "--reviews"])?;
    let (Some(base), Some(event)) = (options.get("--base"), options.get("--event")) else {
        bail!(USAGE);
    };
    match event::read(&file(event)?)? {
        Event::PullRequest(request) => {
            let reviews = match options.get("--reviews") {
                Some(path) => Some(event::reviews(&file(path)?)?),
                None => None,
            };
            pull_request(repo, base, &request, reviews.as_deref())
        }
        Event::Push { after } => push(repo, base, &after),
    }
}

fn on_checkout(repo: &Repo, args: &[String]) -> Result<Report> {
    let options = options(args, &["--ticket", "--base"])?;
    let Some(id) = options.get("--ticket").filter(|id| ticket::is_id(id)) else {
        bail!(USAGE);
    };
    checkout(
        repo,
        options.get("--base").copied().unwrap_or(DEFAULT_BASE),
        id,
    )
}

fn pull_request(
    repo: &Repo,
    base: &str,
    request: &PullRequest,
    reviews: Option<&[Review]>,
) -> Result<Report> {
    let base = repo.commit(base)?;
    let named_by_the_event = repo.commit(&request.base)?;
    if repo.fork(&named_by_the_event, &base)? != named_by_the_event {
        bail!("the base commit the event names is not in the history of --base");
    }
    let head = repo.commit(&request.head)?;
    let fork = repo.fork(&base, &head)?;
    // A base that holds the head would leave no diff and no commit to judge, and every rule would pass on nothing.
    if fork == head {
        bail!("the head commit is already in the history of --base, so there is nothing to judge");
    }
    let tree = Tree::Commit(&head);
    let changes = repo.changes(&fork, &tree)?;
    let commits = repo.commits(&[&format!("{base}..{head}")])?;
    let on_base = repo.entries(&base, ticket::FOLDER)?;
    let mut report = Report::about(format!(
        "a pull request, head {} on base {}: {} changed paths, {} commits",
        short(&head),
        short(&base),
        changes.len(),
        commits.len()
    ));

    let named = ticket::named(&request.branch);
    let ticket = named_by_the_branch(&mut report, repo, &on_base, named.as_deref(), &changes)?;
    in_scope(&mut report, ticket.as_ref(), &changes);
    report.add(Rule::Immutable, paths::immutable(&changes, &on_base));
    report.add(Rule::Apart, paths::apart(&changes));

    let rows = owners_rows(repo, &base, &fork, &tree, &changes)?;
    let mut verdict = Verdict::of(Rule::OwnersRows);
    let approved = reviews.is_some_and(|reviews| event::approved(reviews, request.owner, &head));
    if approved && !rows.is_empty() {
        verdict.notes.push(format!(
            "the owner approved the head commit {}, so these pass:",
            short(&head)
        ));
        verdict.notes.extend(rows);
    } else if !rows.is_empty() {
        verdict.problems = rows;
        verdict.problems.push(format!(
            "these pass only with the owner's approval of the head commit {}: {}",
            short(&head),
            match reviews {
                Some(_) => "the owner's last review is not an approval of it",
                None => "no reviews were given (--reviews)",
            }
        ));
    }
    report.verdicts.push(verdict);

    let mut credits = in_messages(&commits);
    credits.extend(in_text("the pull request's title", &request.title));
    credits.extend(in_text("the pull request's body", &request.body));
    report.add(Rule::Attribution, credits);
    declared(&mut report, repo, &fork, &tree, ticket.as_ref(), &changes)?;
    report.add(Rule::Evidence, evidence::check(&request.body));
    Ok(report)
}

// Everything but the new commits' messages was judged on the pull request, on the same diff, and the ruleset lets nothing reach main another way. The squash message is the one thing written after that.
fn push(repo: &Repo, base: &str, after: &str) -> Result<Report> {
    let head = repo.commit(after)?;
    let commits = if event::is_no_commit(base) {
        repo.commits(&["-1", &head])?
    } else {
        repo.commits(&[&format!("{}..{head}", repo.commit(base)?)])?
    };
    let mut report = Report::about(format!(
        "a push, {} commits up to {}",
        commits.len(),
        short(&head)
    ));
    report.add(Rule::Attribution, in_messages(&commits));
    Ok(report)
}

fn checkout(repo: &Repo, base: &str, id: &str) -> Result<Report> {
    let base = repo.commit(base)?;
    let fork = repo.fork(&base, "HEAD")?;
    let changes = repo.changes(&fork, &Tree::Checkout)?;
    let commits = repo.commits(&[&format!("{base}..HEAD")])?;
    let on_base = repo.entries(&base, ticket::FOLDER)?;
    let mut report = Report::about(format!(
        "this checkout on base {}: {} changed paths, {} commits",
        short(&base),
        changes.len(),
        commits.len()
    ));

    let ticket = named_by_the_caller(&mut report, repo, &on_base, id)?;
    in_scope(&mut report, ticket.as_ref(), &changes);
    report.add(Rule::Immutable, paths::immutable(&changes, &on_base));
    report.add(Rule::Apart, paths::apart(&changes));
    report.add(Rule::Attribution, in_messages(&commits));
    declared(
        &mut report,
        repo,
        &fork,
        &Tree::Checkout,
        ticket.as_ref(),
        &changes,
    )?;
    Ok(report)
}

fn on_the_base(repo: &Repo, entry: &Entry, verdict: &mut Verdict) -> Result<Option<Ticket>> {
    let path = &entry.path;
    if !entry.file {
        verdict
            .problems
            .push(format!("{path} on the base is not a regular file"));
        return Ok(None);
    }
    let id = ticket::id_of(path).unwrap_or_default();
    match ticket::parse(id, &repo.blob(&entry.object)?) {
        Ok(ticket) => {
            verdict.notes.push(format!(
                "{id} is `kind: {}`, as {path} on the base says",
                ticket.kind
            ));
            Ok(Some(ticket))
        }
        Err(error) => {
            verdict.problems.push(format!(
                "{path} on the base cannot be read as a ticket: {error}"
            ));
            Ok(None)
        }
    }
}

fn named_by_the_branch(
    report: &mut Report,
    repo: &Repo,
    on_base: &[Entry],
    named: Option<&str>,
    changes: &[Change],
) -> Result<Option<Ticket>> {
    let mut verdict = Verdict::of(Rule::Ticket);
    let found = named.map_or_else(Vec::new, |id| ticket::on_base(on_base, id));
    let outside: Vec<&Change> = changes
        .iter()
        .filter(|change| !change.path.starts_with("plan/"))
        .collect();
    let ticket = match (found.as_slice(), outside.first()) {
        ([entry], _) => on_the_base(repo, entry, &mut verdict)?,
        ([], None) => {
            verdict.notes.push(NO_TICKET_NEEDED.to_owned());
            None
        }
        ([], Some(first)) => {
            let why = match named {
                Some(_) => {
                    "the branch names a ticket that has no file in plan/tickets/ on the base"
                }
                None => "the branch does not name a ticket (`<milestone>/<ticket>-...`)",
            };
            verdict.problems.push(format!(
                "no ticket: {why}, and without a ticket only paths under plan/ may change; {} are elsewhere, the first: {}",
                outside.len(),
                paths::shown(&first.path)
            ));
            None
        }
        (several, _) => {
            verdict.problems.push(several_named(several.len()));
            None
        }
    };
    report.verdicts.push(verdict);
    Ok(ticket)
}

fn named_by_the_caller(
    report: &mut Report,
    repo: &Repo,
    on_base: &[Entry],
    id: &str,
) -> Result<Option<Ticket>> {
    let mut verdict = Verdict::of(Rule::Ticket);
    let ticket = match ticket::on_base(on_base, id).as_slice() {
        [entry] => on_the_base(repo, entry, &mut verdict)?,
        [] => {
            verdict
                .problems
                .push(format!("{id} has no file in plan/tickets/ on the base"));
            None
        }
        several => {
            verdict.problems.push(several_named(several.len()));
            None
        }
    };
    match repo.branch()? {
        None => verdict
            .problems
            .push(format!("HEAD is on no branch, so no branch names {id}")),
        Some(branch)
            if !ticket::named(&branch).is_some_and(|named| named.eq_ignore_ascii_case(id)) =>
        {
            verdict.problems.push(format!(
                "the branch does not name {id} (`<milestone>/<ticket>-...`)"
            ));
        }
        Some(_) => {}
    }
    report.verdicts.push(verdict);
    Ok(ticket)
}

fn several_named(count: usize) -> String {
    format!("{count} ticket files on the base have that name, and differ only in case")
}

fn in_scope(report: &mut Report, ticket: Option<&Ticket>, changes: &[Change]) {
    match ticket {
        Some(ticket) => report.add(Rule::Scope, ticket::scope(ticket, changes)),
        None => report.note(Rule::Scope, NOT_JUDGED),
    }
}

fn owners_rows(
    repo: &Repo,
    base: &str,
    fork: &str,
    head: &Tree,
    changes: &[Change],
) -> Result<Vec<String>> {
    let fork = Tree::Commit(fork);
    let mut problems = rows::spellings(changes);
    if rows::touched(changes, rows::TRIAGE) {
        problems.extend(rows::triage(
            &repo.read(&fork, rows::TRIAGE)?,
            &repo.read(head, rows::TRIAGE)?,
            &repo.read(&Tree::Commit(base), rows::TRIAGE)?,
        ));
    }
    if rows::touched(changes, rows::WAIVERS) {
        problems.extend(rows::waivers(
            &repo.read(&fork, rows::WAIVERS)?,
            &repo.read(head, rows::WAIVERS)?,
            &repo.read(&Tree::Commit(base), rows::WAIVERS)?,
        ));
    }
    Ok(problems)
}

fn declared(
    report: &mut Report,
    repo: &Repo,
    fork: &str,
    head: &Tree,
    ticket: Option<&Ticket>,
    changes: &[Change],
) -> Result<()> {
    let Some(ticket) = ticket else {
        report.note(Rule::Dependencies, NOT_JUDGED);
        return Ok(());
    };
    let problems = dependencies::check(ticket, changes, || {
        Ok((
            repo.read(&Tree::Commit(fork), dependencies::MANIFEST)?,
            repo.read(head, dependencies::MANIFEST)?,
        ))
    })?;
    report.add(Rule::Dependencies, problems);
    Ok(())
}

fn in_messages(commits: &[Commit]) -> Vec<String> {
    commits
        .iter()
        .flat_map(|commit| in_text(&format!("commit {}", short(&commit.id)), &commit.message))
        .collect()
}

fn in_text(what: &str, text: &str) -> Vec<String> {
    attribution::lines(text)
        .into_iter()
        .map(|(line, kind)| format!("{what}, line {line}: {kind}"))
        .collect()
}
