use anyhow::{Context, Result, bail};
use serde::Deserialize;

const APPROVED: &str = "APPROVED";
const NO_VERDICT: [&str; 2] = ["COMMENTED", "PENDING"];

pub enum Event {
    PullRequest(PullRequest),
    Push { after: String },
}

pub struct PullRequest {
    pub branch: String,
    pub title: String,
    pub body: String,
    pub base: String,
    pub head: String,
    pub owner: u64,
}

#[derive(Deserialize)]
pub struct Review {
    id: u64,
    user: Option<Account>,
    state: String,
    commit_id: Option<String>,
    submitted_at: Option<String>,
}

#[derive(Deserialize)]
struct Account {
    id: u64,
}

#[derive(Deserialize)]
struct Written {
    pull_request: Option<Request>,
    repository: Option<Repository>,
    #[serde(rename = "ref")]
    reference: Option<String>,
    after: Option<String>,
}

#[derive(Deserialize)]
struct Request {
    title: Option<String>,
    body: Option<String>,
    base: Side,
    head: Side,
}

#[derive(Deserialize)]
struct Side {
    sha: String,
    #[serde(rename = "ref")]
    branch: String,
}

#[derive(Deserialize)]
struct Repository {
    owner: Option<Account>,
    default_branch: Option<String>,
}

pub fn is_commit_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn is_no_commit(id: &str) -> bool {
    is_commit_id(id) && id.bytes().all(|byte| byte == b'0')
}

// The file holds the pull request's own words, so an error names a place in it and never quotes it.
pub fn read(text: &str) -> Result<Event> {
    let written: Written = serde_json::from_str(text).map_err(|error| {
        anyhow::anyhow!(
            "the event file is not an event as GitHub writes one (line {}, column {})",
            error.line(),
            error.column()
        )
    })?;
    if let Some(request) = written.pull_request {
        if !is_commit_id(&request.base.sha) || !is_commit_id(&request.head.sha) {
            bail!("the event's base or head is not a commit id");
        }
        let repository = written.repository;
        let about = repository
            .and_then(|repository| Some((repository.owner?.id, repository.default_branch?)));
        let (owner, default_branch) =
            about.context("the event does not name the repository's owner and default branch")?;
        // A check is recorded on the head commit, whatever branch the pull request targets: one judged against a side branch would count on the same head when it is pointed at the default branch.
        if request.base.branch != default_branch {
            bail!(
                "the pull request does not target the default branch, so a pass here would say nothing about a merge there"
            );
        }
        // The repository is under a personal account, so its owner is the one person whose approval counts.
        return Ok(Event::PullRequest(PullRequest {
            branch: request.head.branch,
            title: request.title.unwrap_or_default(),
            body: request.body.unwrap_or_default(),
            base: request.base.sha,
            head: request.head.sha,
            owner,
        }));
    }
    match (written.reference, written.after) {
        (Some(_), Some(after)) if is_commit_id(&after) => Ok(Event::Push { after }),
        _ => bail!("the event is neither a pull request nor a push"),
    }
}

pub fn reviews(text: &str) -> Result<Vec<Review>> {
    let mut reviews = Vec::new();
    let mut pages = 0;
    for page in serde_json::Deserializer::from_str(text).into_iter::<Vec<Review>>() {
        let page = page.map_err(|error| {
            anyhow::anyhow!(
                "the reviews file is not a list of reviews as the API returns one (line {}, column {})",
                error.line(),
                error.column()
            )
        })?;
        reviews.extend(page);
        pages += 1;
    }
    if pages == 0 {
        bail!("the reviews file is empty");
    }
    Ok(reviews)
}

// The owner's last word decides: an approval counts only while nothing of the owner's follows it (a request for changes, or the approval itself dismissed), and only for the commit it was given on.
pub fn approved(reviews: &[Review], owner: u64, head: &str) -> bool {
    reviews
        .iter()
        .filter(|review| review.user.as_ref().is_some_and(|user| user.id == owner))
        .filter(|review| !NO_VERDICT.contains(&review.state.as_str()))
        .max_by_key(|&review| (review.submitted_at.as_deref(), review.id))
        .is_some_and(|last| last.state == APPROVED && last.commit_id.as_deref() == Some(head))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "1111111111111111111111111111111111111111";
    const OLDER: &str = "2222222222222222222222222222222222222222";
    const OWNER: u64 = 7;

    fn review(id: u64, user: u64, state: &str, commit: &str, minute: u64) -> String {
        format!(
            r#"{{"id": {id}, "node_id": "x", "user": {{"login": "someone", "id": {user}, "type": "User"}}, "body": "", "state": "{state}", "commit_id": "{commit}", "submitted_at": "2026-11-14T10:{minute:02}:00Z"}}"#
        )
    }

    fn decided(reviews: &[String]) -> bool {
        let text = format!("[{}]", reviews.join(","));
        approved(&super::reviews(&text).unwrap(), OWNER, HEAD)
    }

    #[test]
    fn the_owners_approval_of_the_head_commit_counts() {
        assert!(decided(&[review(1, OWNER, "APPROVED", HEAD, 1)]));
        assert!(decided(&[
            review(1, OWNER, "CHANGES_REQUESTED", HEAD, 1),
            review(2, OWNER, "APPROVED", HEAD, 2),
        ]));
        assert!(decided(&[
            review(1, OWNER, "APPROVED", HEAD, 1),
            review(2, OWNER, "COMMENTED", HEAD, 2),
            review(3, 8, "CHANGES_REQUESTED", HEAD, 3),
        ]));
        assert!(decided(&[
            review(2, OWNER, "APPROVED", HEAD, 2),
            review(1, OWNER, "CHANGES_REQUESTED", HEAD, 1),
        ]));
    }

    #[test]
    fn nothing_else_counts() {
        assert!(!decided(&[]));
        assert!(!decided(&[review(1, 8, "APPROVED", HEAD, 1)]));
        assert!(!decided(&[review(1, OWNER, "APPROVED", OLDER, 1)]));
        assert!(!decided(&[review(1, OWNER, "COMMENTED", HEAD, 1)]));
        assert!(!decided(&[review(1, OWNER, "approved", HEAD, 1)]));
        assert!(!decided(&[review(1, OWNER, "DISMISSED", HEAD, 1)]));
        assert!(!decided(&[
            review(1, OWNER, "APPROVED", HEAD, 1),
            review(2, OWNER, "CHANGES_REQUESTED", HEAD, 2),
        ]));
        assert!(!decided(&[
            review(1, OWNER, "APPROVED", HEAD, 1),
            review(2, OWNER, "DISMISSED", HEAD, 2),
        ]));
        assert!(!decided(&[
            review(1, OWNER, "APPROVED", HEAD, 1),
            review(2, OWNER, "SOMETHING_NEW", HEAD, 2),
        ]));
        assert!(!decided(&[
            review(1, OWNER, "APPROVED", HEAD, 1),
            review(2, OWNER, "APPROVED", OLDER, 2),
        ]));
        assert!(!decided(&[
            review(1, OWNER, "APPROVED", OLDER, 1),
            review(2, 8, "APPROVED", HEAD, 2),
        ]));
    }

    #[test]
    fn of_two_reviews_at_one_moment_the_later_one_decides() {
        assert!(decided(&[
            review(1, OWNER, "CHANGES_REQUESTED", HEAD, 5),
            review(2, OWNER, "APPROVED", HEAD, 5),
        ]));
        assert!(!decided(&[
            review(2, OWNER, "CHANGES_REQUESTED", HEAD, 5),
            review(1, OWNER, "APPROVED", HEAD, 5),
        ]));
    }

    #[test]
    fn a_review_that_is_not_yet_submitted_decides_nothing() {
        let pending = format!(
            r#"{{"id": 9, "user": {{"id": {OWNER}}}, "state": "PENDING", "commit_id": "{HEAD}", "submitted_at": "2026-11-14T10:59:00Z"}}"#
        );
        assert!(decided(&[
            review(1, OWNER, "APPROVED", HEAD, 1),
            pending.clone()
        ]));
        assert!(!decided(&[pending]));
    }

    #[test]
    fn a_review_with_no_user_or_no_commit_is_read_and_does_not_count() {
        let text = r#"[{"id": 1, "user": null, "state": "APPROVED", "commit_id": null, "submitted_at": null},
            {"id": 2, "user": {"id": 7}, "state": "APPROVED", "commit_id": null}]"#;
        let reviews = reviews(text).unwrap();
        assert_eq!(reviews.len(), 2);
        assert!(!approved(&reviews, OWNER, HEAD));
    }

    #[test]
    fn pages_one_after_another_are_one_list() {
        let pages = format!(
            "[{}]\n[{}]\n[]",
            review(1, OWNER, "APPROVED", HEAD, 1),
            review(2, OWNER, "CHANGES_REQUESTED", HEAD, 2)
        );
        let reviews = reviews(&pages).unwrap();
        assert_eq!(reviews.len(), 2);
        assert!(!approved(&reviews, OWNER, HEAD));
        assert_eq!(super::reviews("[]").unwrap().len(), 0);
    }

    #[test]
    fn a_reviews_file_that_is_not_a_list_is_an_error() {
        let held = "held-in-the-file";
        for text in [
            String::new(),
            "  \n".to_owned(),
            format!(r#"{{"message": "{held}"}}"#),
            format!(r#"[{{"id": "{held}"}}]"#),
            format!("[] {held}"),
        ] {
            let error = reviews(&text).err().expect("not a list").to_string();
            assert!(!error.contains(held), "{error}");
        }
    }

    fn pull_request(body: &str) -> String {
        format!(
            r#"{{"action": "synchronize", "number": 9, "pull_request": {{"number": 9, "title": "M1-F1: a title", "body": {body},
            "base": {{"ref": "main", "sha": "{OLDER}", "repo": {{"id": 1}}}},
            "head": {{"ref": "m1/f1-workspace", "sha": "{HEAD}", "repo": {{"id": 2}}}}}},
            "repository": {{"id": 1, "name": "x", "default_branch": "main", "owner": {{"login": "someone", "id": {OWNER}, "type": "User"}}}}}}"#
        )
    }

    #[test]
    fn a_pull_request_event_is_read() {
        let Event::PullRequest(request) = read(&pull_request("\"## Summary\\r\\n\"")).unwrap()
        else {
            panic!("not a pull request");
        };
        assert_eq!(request.branch, "m1/f1-workspace");
        assert_eq!(request.title, "M1-F1: a title");
        assert_eq!(request.body, "## Summary\r\n");
        assert_eq!(
            (request.base.as_str(), request.head.as_str()),
            (OLDER, HEAD)
        );
        assert_eq!(request.owner, OWNER);
        let Event::PullRequest(request) = read(&pull_request("null")).unwrap() else {
            panic!("not a pull request");
        };
        assert_eq!(request.body, "");
    }

    #[test]
    fn a_pull_request_into_another_branch_is_an_error() {
        for branch in ["side", "Main", "main2", "refs/heads/main", ""] {
            let event = pull_request("\"\"").replace(
                "\"base\": {\"ref\": \"main\"",
                &format!("\"base\": {{\"ref\": \"{branch}\""),
            );
            let error = read(&event).err().expect("another target").to_string();
            assert_eq!(
                error,
                "the pull request does not target the default branch, so a pass here would say nothing about a merge there",
                "{branch}"
            );
        }
        let renamed = pull_request("\"\"")
            .replace("\"ref\": \"main\"", "\"ref\": \"trunk\"")
            .replace(
                "\"default_branch\": \"main\"",
                "\"default_branch\": \"trunk\"",
            );
        assert!(read(&renamed).is_ok());
    }

    #[test]
    fn a_push_event_is_read() {
        let push = format!(
            r#"{{"ref": "refs/heads/main", "before": "{OLDER}", "after": "{HEAD}", "repository": {{"owner": {{"id": {OWNER}}}}}}}"#
        );
        let Event::Push { after } = read(&push).unwrap() else {
            panic!("not a push");
        };
        assert_eq!(after, HEAD);
    }

    #[test]
    fn any_other_file_is_an_error_that_quotes_nothing() {
        let held = "held-in-the-file";
        for text in [
            String::new(),
            "{}".to_owned(),
            format!(r#"{{"action": "{held}", "issue": {{}}}}"#),
            format!(r#"{{"ref": "refs/heads/main", "after": "{held}"}}"#),
            format!(r#"{{"pull_request": "{held}"}}"#),
            pull_request("\"\"").replace(HEAD, held),
            pull_request("\"\"").replace("\"repository\"", "\"other\""),
            pull_request("\"\"").replace("\"default_branch\"", "\"other\""),
            pull_request("\"\"")
                .replace(&format!("\"id\": {OWNER}"), &format!("\"id\": \"{held}\"")),
        ] {
            let error = read(&text).err().expect("not an event").to_string();
            assert!(!error.contains(held), "{error}");
        }
    }

    #[test]
    fn only_forty_or_sixty_four_hex_digits_are_a_commit_id() {
        assert!(is_commit_id(HEAD));
        assert!(is_commit_id(&"a".repeat(64)));
        assert!(is_no_commit(&"0".repeat(40)));
        assert!(!is_no_commit(HEAD));
        for text in ["", "main", "-x", &"a".repeat(39), &"g".repeat(40), "HEAD~1"] {
            assert!(!is_commit_id(text), "{text}");
        }
    }
}
