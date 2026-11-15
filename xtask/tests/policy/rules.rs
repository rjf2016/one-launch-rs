use super::super::git::short;
use super::super::{
    NO_TICKET_NEEDED, NOT_JUDGED, Rule, checkout, on_checkout, on_event, pull_request, push,
};
use super::{BODY, Fixture, OWNER, failed, judged_rules, notes, problems, reviews, ticket};

type Files<'a> = &'a [(&'a str, Option<&'a str>)];

const NONE: [Rule; 0] = [];
const TRAILER: &str = "Co-authored-by: Someone <someone@example.com>";

fn args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

#[test]
fn rule_1_the_branch_names_a_ticket_that_is_on_the_base() {
    let (fixture, base) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        (
            "plan/tickets/M1-F1.md",
            &ticket("feature", &["crates/ol-model/**"]),
        ),
        (
            "plan/tickets/M1-F3.md",
            "---\nid: M1-F3\nwrite_scope: []\n---\n",
        ),
        ("plan/tickets/TEMPLATE.md", &ticket("feature", &["**"])),
        ("plan/PLAN.md", "The plan.\n"),
        ("crates/ol-model/src/lib.rs", "pub fn one() {}\n"),
    ]);
    let code = fixture.head(
        &base,
        &[("crates/ol-model/src/lib.rs", Some("pub fn two() {}\n"))],
        "M1-F1: a change",
    );
    for branch in ["m1/f1-model", "M1/F1-model", "m1/f1-a-b/c"] {
        let report = fixture.judged(branch, &base, &code);
        assert_eq!(failed(&report), NONE, "{branch}");
        assert_eq!(
            notes(&report, Rule::Ticket),
            ["M1-F1 is `kind: feature`, as plan/tickets/M1-F1.md on the base says"]
        );
    }

    let elsewhere = "and without a ticket only paths under plan/ may change; 1 are elsewhere, the first: crates/ol-model/src/lib.rs";
    for (branch, why) in [
        (
            "main",
            "the branch does not name a ticket (`<milestone>/<ticket>-...`)",
        ),
        (
            "m1-f1-model",
            "the branch does not name a ticket (`<milestone>/<ticket>-...`)",
        ),
        (
            "fix/typo-in-the-model",
            "the branch names a ticket that has no file in plan/tickets/ on the base",
        ),
        (
            "m1/f2-model",
            "the branch names a ticket that has no file in plan/tickets/ on the base",
        ),
    ] {
        let report = fixture.judged(branch, &base, &code);
        assert_eq!(failed(&report), [Rule::Ticket], "{branch}");
        assert_eq!(
            problems(&report, Rule::Ticket),
            [format!("no ticket: {why}, {elsewhere}")]
        );
        assert_eq!(notes(&report, Rule::Scope), [NOT_JUDGED]);
        assert_eq!(notes(&report, Rule::Dependencies), [NOT_JUDGED]);
    }

    let unreadable = fixture.judged("m1/f3-model", &base, &code);
    assert_eq!(
        problems(&unreadable, Rule::Ticket),
        ["plan/tickets/M1-F3.md on the base cannot be read as a ticket: it has no `kind`"]
    );

    let own_ticket = fixture.head(
        &base,
        &[
            ("plan/tickets/M1-F9.md", Some(&ticket("interface", &["**"]))),
            ("crates/ol-model/src/lib.rs", Some("pub fn two() {}\n")),
        ],
        "M1-F9: a ticket and its code in one",
    );
    let report = fixture.judged("m1/f9-model", &base, &own_ticket);
    assert_eq!(failed(&report), [Rule::Ticket, Rule::Immutable]);
}

#[test]
fn rule_1_a_pull_request_that_stays_under_plan_needs_no_ticket() {
    let (fixture, base) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        (
            "plan/tickets/M1-F1.md",
            &ticket("feature", &["crates/ol-model/**"]),
        ),
        ("plan/PLAN.md", "The plan.\n"),
        ("docs/guide.md", "A guide.\n"),
    ]);
    let plan_only = fixture.head(
        &base,
        &[
            ("plan/PLAN.md", Some("The plan, changed.\n")),
            ("plan/tickets/M2-001.md", Some(&ticket("feature", &["**"]))),
            ("plan/decisions/M1-x.md", Some("Decided.\n")),
        ],
        "Plan: a change",
    );
    for branch in ["main", "plan/tickets-m2", "m2/001-first"] {
        let report = fixture.judged(branch, &base, &plan_only);
        assert_eq!(failed(&report), NONE, "{branch}");
        assert_eq!(notes(&report, Rule::Ticket), [NO_TICKET_NEEDED]);
        assert_eq!(
            judged_rules(&report),
            [
                Rule::Ticket,
                Rule::Scope,
                Rule::Immutable,
                Rule::Apart,
                Rule::OwnersRows,
                Rule::Attribution,
                Rule::Dependencies,
                Rule::Evidence
            ]
        );
    }
    let named = fixture.judged("m1/f1-model", &base, &plan_only);
    assert_eq!(failed(&named), [Rule::Scope]);

    let with_one_more = fixture.head(
        &base,
        &[
            ("plan/PLAN.md", Some("The plan, changed.\n")),
            ("docs/guide.md", Some("A guide, changed.\n")),
        ],
        "Plan: a change, and a page",
    );
    let report = fixture.judged("plan/tickets-m2", &base, &with_one_more);
    assert_eq!(failed(&report), [Rule::Ticket]);
    assert_eq!(
        problems(&report, Rule::Ticket),
        [
            "no ticket: the branch names a ticket that has no file in plan/tickets/ on the base, and without a ticket only paths under plan/ may change; 1 are elsewhere, the first: docs/guide.md"
        ]
    );
    let another_case = fixture.staged(&base, &[("100644", "Plan/notes.md")], "Plan: a note");
    let report = fixture.judged("main", &base, &another_case);
    assert_eq!(failed(&report), [Rule::Ticket]);
    let an_odd_name = fixture.head_with_a_name(&base, "q\"x\n::notice::y.md");
    let report = fixture.judged("main", &base, &an_odd_name);
    assert_eq!(
        problems(&report, Rule::Ticket),
        [
            "no ticket: the branch does not name a ticket (`<milestone>/<ticket>-...`), and without a ticket only paths under plan/ may change; 1 are elsewhere, the first: q\"x?::notice::y.md"
        ]
    );

    let still_judged = fixture.head(
        &base,
        &[("plan/tickets/M1-F1.md", Some(&ticket("interface", &["**"])))],
        &format!("Plan: a wider ticket\n\n{TRAILER}"),
    );
    let report = fixture.judged("main", &base, &still_judged);
    assert_eq!(failed(&report), [Rule::Immutable, Rule::Attribution]);
}

#[test]
fn rule_1_two_tickets_that_differ_only_in_case_name_none() {
    let (fixture, first) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        ("plan/tickets/M1-F1.md", &ticket("feature", &["**"])),
    ]);
    let base = fixture.staged(
        &first,
        &[("100644", "plan/tickets/m1-f1.md")],
        "A second spelling",
    );
    let head = fixture.staged(
        &base,
        &[("100644", "crates/a/src/lib.rs")],
        "M1-F1: a change",
    );
    let report = fixture.judged("m1/f1-model", &base, &head);
    assert_eq!(failed(&report), [Rule::Ticket]);
    assert_eq!(
        problems(&report, Rule::Ticket),
        ["2 ticket files on the base have that name, and differ only in case"]
    );
}

#[test]
fn rule_2_every_changed_path_is_inside_the_write_scope() {
    let scope = [
        "crates/ol-model/**",
        "docs/guide.md          # a comment after the entry",
        "crates/**/tests/*.rs",
        "notes/**",
    ];
    let (fixture, base) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        ("plan/tickets/M1-F1.md", &ticket("feature", &scope)),
        ("plan/tickets/M1-F2.md", &ticket("interface", &scope)),
        ("plan/tickets/M1-F3.md", &ticket("feature", &["**"])),
        ("crates/ol-model/src/lib.rs", "pub fn one() {}\n"),
        ("crates/ol-model/src/old.rs", "pub fn old() {}\n"),
        ("crates/ol-store/src/db.rs", "pub fn db() {}\n"),
        ("docs/guide.md", "A guide.\n"),
        ("docs/other.md", "Another page.\n"),
        ("tools/run.sh", "echo run\n"),
    ]);
    let outside = |head: &str| {
        let report = fixture.judged("m1/f1-model", &base, head);
        assert_eq!(failed(&report), [Rule::Scope]);
        problems(&report, Rule::Scope)
    };
    let message = |path: &str| format!("outside the write_scope of M1-F1: {path}");

    let inside = fixture.head(
        &base,
        &[
            ("crates/ol-model/src/lib.rs", Some("pub fn two() {}\n")),
            ("crates/ol-model/src/old.rs", None),
            (
                "crates/ol-model/src/a new file.rs",
                Some("pub fn new() {}\n"),
            ),
            ("docs/guide.md", Some("A better guide.\n")),
            ("crates/ol-store/tests/db.rs", Some("#[test]\nfn db() {}\n")),
        ],
        "M1-F1: inside the scope",
    );
    assert_eq!(failed(&fixture.judged("m1/f1-model", &base, &inside)), NONE);

    let changes: [(Files, &str); 7] = [
        (&[("docs/other.md", Some("Changed.\n"))], "docs/other.md"),
        (&[("tools/run.sh", None)], "tools/run.sh"),
        (
            &[("docs/a new page.md", Some("New.\n"))],
            "docs/a new page.md",
        ),
        (
            &[
                ("crates/ol-model/src/old.rs", None),
                ("docs/old.rs", Some("pub fn old() {}\n")),
            ],
            "docs/old.rs",
        ),
        (
            &[
                ("docs/other.md", None),
                ("crates/ol-model/other.md", Some("Another page.\n")),
            ],
            "docs/other.md",
        ),
        (
            &[("crates/ol-store/tests/deep/db.rs", Some("fn db() {}\n"))],
            "crates/ol-store/tests/deep/db.rs",
        ),
        (
            &[("crates/ol-store/src/db.rs", Some("pub fn db2() {}\n"))],
            "crates/ol-store/src/db.rs",
        ),
    ];
    for (files, path) in changes {
        let head = fixture.head(&base, files, "M1-F1: one path outside the scope");
        assert_eq!(outside(&head), [message(path)]);
    }

    for (mode, path) in [
        ("100644", "Crates/ol-model/src/new.rs"),
        ("120000", "docs/link"),
        ("160000", "vendor/sub"),
    ] {
        let head = fixture.staged(&base, &[(mode, path)], "M1-F1: an entry of another kind");
        assert_eq!(outside(&head), [message(path)]);
    }
    for (mode, path) in [("120000", "notes/link"), ("160000", "notes/sub")] {
        let head = fixture.staged(&base, &[(mode, path)], "M1-F1: an entry of another kind");
        assert_eq!(failed(&fixture.judged("m1/f1-model", &base, &head)), NONE);
    }
    let executable = fixture.executable(&base, "tools/run.sh", "M1-F1: only the mode");
    assert_eq!(outside(&executable), [message("tools/run.sh")]);

    let backslash = fixture.head_with_a_name(&base, "a\\b.md");
    assert_eq!(outside(&backslash), [message("a\\b.md")]);
    let quote_and_newline = fixture.head_with_a_name(&base, "q\"x\ny.md");
    assert_eq!(outside(&quote_and_newline), [message("q\"x?y.md")]);
    let anything = |head: &str| failed(&fixture.judged("m1/f3-anything", &base, head));
    assert_eq!(anything(&quote_and_newline), NONE);
    assert_eq!(anything(&backslash), [Rule::Scope]);

    let widened = fixture.head(
        &base,
        &[
            ("plan/tickets/M1-F1.md", Some(&ticket("feature", &["**"]))),
            ("docs/other.md", Some("Changed.\n")),
        ],
        "M1-F1: a wider scope for itself",
    );
    let report = fixture.judged("m1/f1-model", &base, &widened);
    assert_eq!(failed(&report), [Rule::Scope, Rule::Immutable]);
    assert_eq!(
        problems(&report, Rule::Scope),
        [message("docs/other.md"), message("plan/tickets/M1-F1.md")]
    );
}

#[test]
fn rule_2_fifteen_files_at_most_unless_the_ticket_is_an_interface() {
    let scope = ["crates/ol-model/**"];
    let (fixture, base) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        ("plan/tickets/M1-F1.md", &ticket("feature", &scope)),
        ("plan/tickets/M1-F2.md", &ticket("interface", &scope)),
        ("crates/ol-model/src/old.rs", "pub fn old() {}\n"),
    ]);
    let files = |count: usize| -> Vec<(String, String)> {
        (0..count)
            .map(|index| {
                (
                    format!("crates/ol-model/src/f{index}.rs"),
                    format!("pub fn f{index}() {{}}\n"),
                )
            })
            .collect()
    };
    let head = |count: usize, renamed: bool| {
        let files = files(count);
        let mut files: Vec<(&str, Option<&str>)> = files
            .iter()
            .map(|(path, text)| (path.as_str(), Some(text.as_str())))
            .collect();
        if renamed {
            files.push(("crates/ol-model/src/old.rs", None));
            files.push(("crates/ol-model/src/new.rs", Some("pub fn old() {}\n")));
        }
        fixture.head(&base, &files, "M1-F1: many files")
    };

    let fifteen = head(15, false);
    assert_eq!(
        failed(&fixture.judged("m1/f1-model", &base, &fifteen)),
        NONE
    );
    let sixteen = head(16, false);
    let report = fixture.judged("m1/f1-model", &base, &sixteen);
    assert_eq!(failed(&report), [Rule::Scope]);
    assert_eq!(
        problems(&report, Rule::Scope),
        [
            "16 files are changed, 15 at most unless the ticket is `kind: interface` (a rename counts as two)"
        ]
    );
    assert_eq!(
        failed(&fixture.judged("m1/f2-model", &base, &sixteen)),
        NONE
    );
    let fourteen_and_a_rename = head(14, true);
    assert_eq!(
        failed(&fixture.judged("m1/f1-model", &base, &fourteen_and_a_rename)),
        [Rule::Scope]
    );
}

#[test]
fn rule_3_a_ticket_on_the_base_does_not_change() {
    let (fixture, base) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        (
            "plan/tickets/M1-F1.md",
            &ticket("feature", &["plan/**", "crates/**"]),
        ),
        ("plan/tickets/M1-F2.md", &ticket("feature", &["docs/**"])),
        ("plan/tickets/TEMPLATE.md", &ticket("feature", &["**"])),
        ("plan/milestones/M2.md", "Milestone 2.\n"),
        ("crates/a/src/lib.rs", "pub fn one() {}\n"),
    ]);
    let changed = "a ticket that is on the base is changed: plan/tickets/M1-F2.md";
    let wider = ticket("interface", &["**"]);

    let new_tickets = fixture.head(
        &base,
        &[
            (
                "plan/tickets/M2-001.md",
                Some(&ticket("feature", &["crates/**"])),
            ),
            (
                "plan/tickets/M2-002.md",
                Some(&ticket("feature", &["docs/**"])),
            ),
            ("plan/tickets/TEMPLATE.md", Some(&ticket("fix", &["**"]))),
            (
                "plan/milestones/M2.md",
                Some("Milestone 2, with tickets.\n"),
            ),
        ],
        "M2: the tickets",
    );
    assert_eq!(
        failed(&fixture.judged("prep/m2-tickets", &base, &new_tickets)),
        NONE
    );

    let cases: [(Files, &str); 3] = [
        (&[("plan/tickets/M1-F2.md", Some(&wider))], "edited"),
        (&[("plan/tickets/M1-F2.md", None)], "removed"),
        (
            &[
                ("plan/tickets/M1-F2.md", None),
                (
                    "plan/tickets/M1-F4.md",
                    Some(&ticket("feature", &["docs/**"])),
                ),
            ],
            "renamed",
        ),
    ];
    for (files, how) in cases {
        let head = fixture.head(&base, files, "Plan: a ticket that was ready");
        let report = fixture.judged("prep/m2-tickets", &base, &head);
        assert_eq!(failed(&report), [Rule::Immutable], "{how}");
        assert_eq!(problems(&report, Rule::Immutable), [changed], "{how}");
    }
    let executable = fixture.executable(&base, "plan/tickets/M1-F2.md", "Plan: only the mode");
    let report = fixture.judged("prep/m2-tickets", &base, &executable);
    assert_eq!(problems(&report, Rule::Immutable), [changed]);
    let respelled = fixture.staged(
        &base,
        &[("100644", "plan/tickets/m1-f2.md")],
        "Plan: the ticket again, in another case",
    );
    let report = fixture.judged("prep/m2-tickets", &base, &respelled);
    assert_eq!(
        problems(&report, Rule::Immutable),
        [
            "a ticket that is on the base is changed under another spelling of its path: plan/tickets/M1-F2.md"
        ]
    );

    let its_own = fixture.head(
        &base,
        &[("plan/tickets/M1-F1.md", Some(&wider))],
        "M1-F1: its own ticket, which its scope names",
    );
    let report = fixture.judged("m1/f1-plan", &base, &its_own);
    assert_eq!(failed(&report), [Rule::Immutable]);

    let with_code = fixture.head(
        &base,
        &[
            (
                "plan/tickets/M2-001.md",
                Some(&ticket("feature", &["crates/**"])),
            ),
            ("crates/a/src/lib.rs", Some("pub fn two() {}\n")),
        ],
        "M1-F1: a new ticket beside code",
    );
    let report = fixture.judged("m1/f1-plan", &base, &with_code);
    assert_eq!(failed(&report), [Rule::Immutable]);
    assert_eq!(
        problems(&report, Rule::Immutable),
        [
            "ticket files are added together with 1 paths outside plan/, the first: crates/a/src/lib.rs"
        ]
    );

    fixture.head(
        &base,
        &[("plan/tickets/M1-F2.md", Some(&wider))],
        "Plan: a ticket changed",
    );
    fixture.write("plan/tickets/M1-F2.md", &ticket("feature", &["docs/**"]));
    fixture.write("plan/milestones/M2.md", "Milestone 2, changed.\n");
    let and_back = fixture.commit("Plan: and changed back");
    let report = fixture.judged("prep/m2-tickets", &base, &and_back);
    assert_eq!(failed(&report), NONE);
    assert!(
        report.about.ends_with("1 changed paths, 2 commits"),
        "{}",
        report.about
    );

    let edited = fixture.head(
        &base,
        &[("plan/tickets/M1-F2.md", Some(&wider))],
        "Plan: a ticket that was ready",
    );
    let without_it = fixture.head(
        &base,
        &[("plan/tickets/M1-F2.md", None)],
        "Plan: the base no longer has the ticket",
    );
    let report = fixture.judged("prep/m2-tickets", &without_it, &edited);
    assert_eq!(failed(&report), [Rule::Immutable]);
    assert_eq!(
        problems(&report, Rule::Immutable),
        ["a ticket that was there when the branch was cut is changed: plan/tickets/M1-F2.md"]
    );
}

#[test]
fn rule_4_the_plan_files_and_the_spec_stay_apart_from_the_crates() {
    let waivers = "# Waivers.\n#   tooling           tests of scripts that do not ship\n\n";
    let row = "[[item]]\nid = \"X-01\"\ndecision = \"fix\"\nsource = \"delegated\"\n\n";
    let waiver = "[[waiver]]\nid = \"T-0001\"\ncategory = \"tooling\"\nreason = \"A script.\"\n";
    let (fixture, base) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        ("plan/tickets/M1-F1.md", &ticket("feature", &["**"])),
        ("plan/triage.toml", row),
        ("plan/waivers.toml", waivers),
        ("spec/SPEC.md", "The spec.\n"),
        ("crates/a/src/lib.rs", "pub fn one() {}\n"),
        ("xtask/src/main.rs", "fn main() {}\n"),
    ]);
    let more_rows = format!("{row}{}", row.replace("X-01", "X-02"));
    let more_waivers = format!("{waivers}{waiver}");
    let code = ("crates/a/src/lib.rs", Some("pub fn two() {}\n"));

    let apart: [Files; 4] = [
        &[
            ("spec/SPEC.md", Some("The spec, amended.\n")),
            ("plan/PLAN.md", Some("The plan.\n")),
        ],
        &[code, ("xtask/src/main.rs", Some("fn main() { run() }\n"))],
        &[
            ("plan/triage.toml", Some(&more_rows)),
            ("xtask/src/main.rs", Some("fn main() { run() }\n")),
        ],
        &[
            ("plan/waivers.toml", Some(&more_waivers)),
            ("docs/crates/a.md", Some("Not a crate.\n")),
        ],
    ];
    for files in apart {
        let head = fixture.head(&base, files, "M1-F1: one side only");
        assert_eq!(failed(&fixture.judged("m1/f1-both", &base, &head)), NONE);
    }

    let together: [(Files, &str); 4] = [
        (
            &[("spec/SPEC.md", Some("The spec, amended.\n")), code],
            "spec/",
        ),
        (&[("spec/vectors/new.json", Some("{}\n")), code], "spec/"),
        (
            &[("plan/triage.toml", Some(&more_rows)), code],
            "plan/triage.toml",
        ),
        (
            &[("plan/waivers.toml", Some(&more_waivers)), code],
            "plan/waivers.toml",
        ),
    ];
    for (files, plan) in together {
        let head = fixture.head(&base, files, "M1-F1: both sides at once");
        let report = fixture.judged("m1/f1-both", &base, &head);
        assert_eq!(failed(&report), [Rule::Apart], "{plan}");
        assert_eq!(
            problems(&report, Rule::Apart),
            [format!(
                "{plan} is changed together with 1 paths under crates/, the first: crates/a/src/lib.rs"
            )]
        );
    }

    let moved = fixture.head(
        &base,
        &[
            ("spec/SPEC.md", None),
            ("crates/a/SPEC.md", Some("The spec.\n")),
        ],
        "M1-F1: the spec moved into a crate",
    );
    assert_eq!(
        failed(&fixture.judged("m1/f1-both", &base, &moved)),
        [Rule::Apart]
    );
    for entries in [
        [("100644", "Spec/new.md"), ("100644", "crates/a/src/new.rs")],
        [("100644", "spec/new.md"), ("100644", "Crates/new.rs")],
        [("120000", "spec/link"), ("160000", "crates/sub")],
    ] {
        let head = fixture.staged(&base, &entries, "M1-F1: both sides, spelled another way");
        let report = fixture.judged("m1/f1-both", &base, &head);
        assert_eq!(failed(&report), [Rule::Apart], "{entries:?}");
    }
}

const CATEGORIES: &str = "# Waivers.\n#\n#   web-only          behaviour of the web page\n#   tooling           tests of scripts that do not ship\n\n";

fn row(id: &str, source: &str, decision: &str) -> String {
    format!(
        "[[item]]\nid = \"{id}\"\nsummary = \"What {id} is about.\"\ndecision = \"{decision}\"\nsource = \"{source}\"\naffects = []\n\n"
    )
}

fn waiver(id: &str, category: &str) -> String {
    format!(
        "[[waiver]]\nid = \"{id}\"\ncategory = \"{category}\"\nreason = \"Why {id} is not ported.\"\n\n"
    )
}

#[test]
fn rule_5_the_owners_rows_change_only_with_the_owners_approval() {
    let triage = format!(
        "# The triage table.\n\n{}{}{}",
        row("S1-01", "", "keep"),
        row("S1-02", "plan", "drop"),
        row("X-01", "delegated", "fix")
    );
    let waivers = format!("{CATEGORIES}{}", waiver("T-0001", "tooling"));
    let (fixture, base) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        ("plan/tickets/M1-F1.md", &ticket("feature", &["plan/**"])),
        ("plan/triage.toml", &triage),
        ("plan/waivers.toml", &waivers),
    ]);
    let judged = |head: &str| fixture.judged("prep/rows-1", &base, head);
    let needs = |head: &str, given: &str| {
        format!(
            "these pass only with the owner's approval of the head commit {}: {given}",
            short(head)
        )
    };
    let no_reviews = "no reviews were given (--reviews)";
    let not_approved = "the owner's last review is not an approval of it";

    let delegated = fixture.head(
        &base,
        &[
            (
                "plan/triage.toml",
                Some(&format!(
                    "{}{}",
                    triage.replace("decision = \"fix\"", "decision = \"drop\""),
                    row("X-02", "delegated", "drop")
                )),
            ),
            (
                "plan/waivers.toml",
                Some(&format!("{waivers}{}", waiver("T-0002", "web-only"))),
            ),
        ],
        "Plan: a delegated row and a waiver",
    );
    assert_eq!(failed(&judged(&delegated)), NONE);
    assert_eq!(
        failed(&fixture.judged("m1/f1-rows", &base, &delegated)),
        NONE
    );

    let hidden = row("X-02", "delegated", "drop").replace(
        "decision = \"drop\"",
        &format!("decision = \"\"\"\n{}\"\"\"", row("S1-02", "plan", "drop")),
    );
    let cases: [(&str, String, &str); 9] = [
        (
            "plan/triage.toml",
            triage.replace("decision = \"keep\"", "decision = \"drop\""),
            "plan/triage.toml:3: the owner's row S1-01 is changed",
        ),
        (
            "plan/triage.toml",
            triage.replace(&row("S1-02", "plan", "drop"), ""),
            "plan/triage.toml: the owner's row S1-02 is removed",
        ),
        (
            "plan/triage.toml",
            format!("{triage}{}", row("S1-03", "plan", "drop")),
            "plan/triage.toml:24: a row with `source` \"\" or \"plan\" is added, and those are the owner's",
        ),
        (
            "plan/triage.toml",
            triage.replace("source = \"delegated\"", "source = \"plan\""),
            "plan/triage.toml:17: a row with `source` \"\" or \"plan\" is added, and those are the owner's",
        ),
        (
            "plan/triage.toml",
            format!(
                "{}{hidden}",
                triage.replace(&row("S1-02", "plan", "drop"), "")
            ),
            "plan/triage.toml:20: not a comment, `[[item]]` or one `key = value` that ends on its line, which is all this check reads",
        ),
        (
            "plan/waivers.toml",
            waivers.replace("that do not ship", "and anything hard"),
            "plan/waivers.toml: the comment above the first waiver, which defines the categories, is changed",
        ),
        (
            "plan/waivers.toml",
            format!("{waivers}{}", waiver("T-0002", "too-hard")),
            "plan/waivers.toml:11: the waiver's category is not one the base defines",
        ),
        (
            "plan/waivers.toml",
            waivers.replace("\"tooling\"", "\"web-only\""),
            "plan/waivers.toml:6: the category of waiver T-0001, which is on the base, is changed",
        ),
        (
            "plan/waivers.toml",
            String::new(),
            "plan/waivers.toml: the comment above the first waiver, which defines the categories, is changed",
        ),
    ];
    for (path, text, problem) in &cases {
        let head = fixture.head(
            &base,
            &[(*path, Some(text.as_str()))],
            "Plan: one of the owner's rows",
        );
        let report = judged(&head);
        assert_eq!(failed(&report), [Rule::OwnersRows], "{problem}");
        assert_eq!(
            problems(&report, Rule::OwnersRows),
            [(*problem).to_owned(), needs(&head, no_reviews)]
        );
    }
    let removed = fixture.head(&base, &[("plan/triage.toml", None)], "Plan: no table");
    assert_eq!(problems(&judged(&removed), Rule::OwnersRows).len(), 3);
    let respelled = fixture.staged(
        &base,
        &[("100644", "Plan/Triage.toml")],
        "Plan: a second table",
    );
    assert_eq!(
        problems(&judged(&respelled), Rule::OwnersRows),
        [
            "plan/triage.toml: a path that is this file under another spelling is changed"
                .to_owned(),
            needs(&respelled, no_reviews)
        ]
    );

    let (path, text, problem) = &cases[0];
    let first = fixture.head(
        &base,
        &[(*path, Some(text.as_str()))],
        "Plan: one of the owner's rows",
    );
    fixture.write("plan/PLAN.md", "A later commit.\n");
    let head = fixture.commit("Plan: a commit after the review");
    let request = fixture.request("prep/rows-1", &base, &head);
    let with = |given: &[(u64, &str, &str)]| {
        pull_request(&fixture.repo, &base, &request, Some(&reviews(given))).unwrap()
    };
    for given in [
        vec![(OWNER, "APPROVED", head.as_str())],
        vec![
            (OWNER, "CHANGES_REQUESTED", head.as_str()),
            (OWNER, "APPROVED", head.as_str()),
        ],
        vec![
            (OWNER, "APPROVED", head.as_str()),
            (OWNER, "COMMENTED", head.as_str()),
            (8, "CHANGES_REQUESTED", head.as_str()),
        ],
    ] {
        let report = with(&given);
        assert_eq!(failed(&report), NONE, "{given:?}");
        assert_eq!(
            notes(&report, Rule::OwnersRows),
            [
                format!(
                    "the owner approved the head commit {}, so these pass:",
                    short(&head)
                ),
                (*problem).to_owned()
            ]
        );
    }
    for given in [
        vec![],
        vec![(OWNER, "APPROVED", first.as_str())],
        vec![(8, "APPROVED", head.as_str())],
        vec![(OWNER, "COMMENTED", head.as_str())],
        vec![(OWNER, "DISMISSED", head.as_str())],
        vec![
            (OWNER, "APPROVED", head.as_str()),
            (OWNER, "CHANGES_REQUESTED", head.as_str()),
        ],
        vec![
            (OWNER, "APPROVED", first.as_str()),
            (8, "APPROVED", head.as_str()),
        ],
    ] {
        let report = with(&given);
        assert_eq!(failed(&report), [Rule::OwnersRows], "{given:?}");
        assert_eq!(
            problems(&report, Rule::OwnersRows),
            [(*problem).to_owned(), needs(&head, not_approved)]
        );
    }

    fixture.on("work", &first);
    fixture.write("plan/PLAN.md", "A later commit.\n");
    let credited = fixture.commit(&format!("Plan: a commit after the review\n\n{TRAILER}"));
    let request = fixture.request("prep/rows-1", &base, &credited);
    let approved = reviews(&[(OWNER, "APPROVED", credited.as_str())]);
    let report = pull_request(&fixture.repo, &base, &request, Some(&approved)).unwrap();
    assert_eq!(failed(&report), [Rule::Attribution]);

    fixture.on("work", &first);
    fixture.write("plan/tickets/M1-F1.md", &ticket("interface", &["**"]));
    let with_a_ticket =
        fixture.commit("Plan: one of the owner's rows, and a ticket that was ready");
    let request = fixture.request("prep/rows-1", &base, &with_a_ticket);
    let approved = reviews(&[(OWNER, "APPROVED", with_a_ticket.as_str())]);
    let report = pull_request(&fixture.repo, &base, &request, Some(&approved)).unwrap();
    assert_eq!(failed(&report), [Rule::Immutable]);
}

#[test]
fn rule_5_a_row_the_base_made_the_owners_after_the_branch_was_cut_is_the_owners() {
    let triage = format!(
        "# The triage table.\n\n{}{}",
        row("S1-01", "", "keep"),
        row("X-01", "delegated", "fix")
    );
    let waivers = format!("{CATEGORIES}{}", waiver("T-0001", "tooling"));
    let (fixture, cut) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        ("plan/triage.toml", &triage),
        ("plan/waivers.toml", &waivers),
    ]);
    let base = fixture.head(
        &cut,
        &[
            (
                "plan/triage.toml",
                Some(&triage.replace("source = \"delegated\"", "source = \"plan\"")),
            ),
            (
                "plan/waivers.toml",
                Some(&format!("{waivers}{}", waiver("T-0002", "tooling"))),
            ),
        ],
        "Plan: the owner takes a row and adds a waiver",
    );
    let judged = |against: &str, head: &str| {
        let report = fixture.judged("prep/rows-2", against, head);
        (failed(&report), problems(&report, Rule::OwnersRows))
    };

    let edited = fixture.head(
        &cut,
        &[(
            "plan/triage.toml",
            Some(&triage.replace("What X-01 is about.", "Something else.")),
        )],
        "Plan: a row that was delegated when the branch was cut",
    );
    let (rules, found) = judged(&base, &edited);
    assert_eq!(rules, [Rule::OwnersRows]);
    assert_eq!(
        found[0],
        "plan/triage.toml:10: the owner's row X-01 is changed"
    );
    assert_eq!(judged(&cut, &edited).0, NONE);

    let removed = fixture.head(
        &cut,
        &[(
            "plan/triage.toml",
            Some(&triage.replace(&row("X-01", "delegated", "fix"), "")),
        )],
        "Plan: the row goes",
    );
    assert_eq!(
        judged(&base, &removed).1[0],
        "plan/triage.toml: the owner's row X-01 is removed"
    );
    assert_eq!(judged(&cut, &removed).0, NONE);

    let beside_it = fixture.head(
        &cut,
        &[(
            "plan/triage.toml",
            Some(&format!("{triage}{}", row("X-02", "delegated", "drop"))),
        )],
        "Plan: another row, the one the base took left alone",
    );
    assert_eq!(judged(&base, &beside_it).0, NONE);

    let twin = fixture.head(
        &cut,
        &[(
            "plan/waivers.toml",
            Some(&format!("{waivers}{}", waiver("T-0002", "web-only"))),
        )],
        "Plan: a waiver the base has since gained",
    );
    assert_eq!(
        judged(&base, &twin).1[0],
        "plan/waivers.toml:11: a waiver is added with the id of waiver T-0002, which the base has"
    );
    assert_eq!(judged(&cut, &twin).0, NONE);
}

#[test]
fn rule_6_no_commit_message_and_no_body_carries_an_attribution() {
    let (fixture, base) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        ("plan/tickets/M1-F1.md", &ticket("feature", &["crates/**"])),
        ("crates/a/src/lib.rs", "pub fn one() {}\n"),
    ]);
    let change = |text: &'static str| [("crates/a/src/lib.rs", Some(text))];
    let judged = |head: &str| fixture.judged("m1/f1-model", &base, head);
    let trailer = "a `Co-authored-by` trailer";

    let plain = fixture.head(
        &base,
        &change("pub fn two() {}\n"),
        "M1-F1: a change - so that GitHub adds no `Co-authored-by` trailer to it\n\nThe default squash message ends with a Co-authored-by: line naming the app.",
    );
    assert_eq!(failed(&judged(&plain)), NONE);
    assert_eq!(failed(&push(&fixture.repo, &base, &plain).unwrap()), NONE);

    let credited = fixture.head(
        &base,
        &change("pub fn two() {}\n"),
        &format!("M1-F1: a change\n\n{TRAILER}"),
    );
    let report = judged(&credited);
    assert_eq!(failed(&report), [Rule::Attribution]);
    let in_the_commit = format!("commit {}, line 3: {trailer}", short(&credited));
    assert_eq!(
        problems(&report, Rule::Attribution),
        [in_the_commit.as_str()]
    );

    let after_the_merge = push(&fixture.repo, &base, &credited).unwrap();
    assert_eq!(judged_rules(&after_the_merge), [Rule::Attribution]);
    assert_eq!(
        problems(&after_the_merge, Rule::Attribution),
        [in_the_commit.as_str()]
    );
    let first_push = push(&fixture.repo, &"0".repeat(40), &credited).unwrap();
    assert_eq!(
        problems(&first_push, Rule::Attribution),
        [in_the_commit.as_str()]
    );

    fixture.on("work", &base);
    let mut expected = Vec::new();
    for (index, (message, kind)) in [
        ("M1-F1: one\n\nCO-AUTHORED-BY : Someone", trailer),
        ("M1-F1: two\n\n  co-authored-by:Someone", trailer),
        (
            "M1-F1: three\n\nGenerated with [Claude Code](https://claude.com/claude-code)",
            "a line that credits an AI tool",
        ),
        (
            "M1-F1: four\n\nhttps://claude.ai/code/session_0123",
            "a link to an AI session",
        ),
        (
            "M1-F1: five\n\nAssisted-by: a tool",
            "a trailer that credits a tool",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        fixture.write("crates/a/src/lib.rs", &format!("pub fn f{index}() {{}}\n"));
        let commit = fixture.commit(message);
        expected.insert(0, format!("commit {}, line 3: {kind}", short(&commit)));
    }
    let several = fixture.git(&["rev-parse", "HEAD"]);
    assert_eq!(problems(&judged(&several), Rule::Attribution), expected);

    fixture.on("side", &base);
    fixture.write("crates/a/src/side.rs", "pub fn side() {}\n");
    let side = fixture.commit(&format!("M1-F1: on a side branch\n\n{TRAILER}"));
    fixture.on("work", &plain);
    fixture.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "Merge the side branch",
        &side,
    ]);
    let merged = fixture.git(&["rev-parse", "HEAD"]);
    assert_eq!(
        problems(&judged(&merged), Rule::Attribution),
        [format!("commit {}, line 3: {trailer}", short(&side))]
    );
    fixture.on("work", &plain);
    let merge_message = format!("Merge the side branch\n\n{TRAILER}");
    fixture.git(&["merge", "-q", "--no-ff", "-m", &merge_message, "side~0"]);
    let merged = fixture.git(&["rev-parse", "HEAD"]);
    assert_eq!(problems(&judged(&merged), Rule::Attribution).len(), 2);

    let mut request = fixture.request("m1/f1-model", &base, &plain);
    request.body = format!("{BODY}\n{TRAILER}\n");
    request.title = "M1-F1: a change, written by Copilot".to_owned();
    let report = pull_request(&fixture.repo, &base, &request, None).unwrap();
    assert_eq!(failed(&report), [Rule::Attribution]);
    assert_eq!(
        problems(&report, Rule::Attribution),
        [
            "the pull request's title, line 1: a line that credits an AI tool".to_owned(),
            format!("the pull request's body, line 14: {trailer}")
        ]
    );

    fixture.on("m1/f1-model", &credited);
    let report = checkout(&fixture.repo, &base, "M1-F1").unwrap();
    assert_eq!(failed(&report), [Rule::Attribution]);
    assert_eq!(problems(&report, Rule::Attribution), [in_the_commit]);
}

const MANIFEST: &str = "[workspace]\nresolver = \"3\"\nmembers = [\"crates/*\"]\n\n[workspace.dependencies]\nserde = { version = \"=1.0.229\", features = [\"derive\"] }\nanyhow = { version = \"=1.0.104\" }\n\n[workspace.lints.cargo]\nunused_dependencies = \"allow\"\n";

#[test]
fn rule_7_dependencies_change_only_in_a_deps_or_interface_ticket() {
    let (fixture, base) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        ("plan/tickets/M1-F1.md", &ticket("feature", &["**"])),
        ("plan/tickets/M1-D1.md", &ticket("deps", &["**"])),
        ("plan/tickets/M1-I1.md", &ticket("interface", &["**"])),
        ("Cargo.toml", MANIFEST),
        ("Cargo.lock", "# The lockfile.\nversion = 4\n"),
        ("deny.toml", "[bans]\ndeny = []\n"),
        ("crates/a/Cargo.toml", "[package]\nname = \"a\"\n"),
    ]);
    let only =
        ", which only a ticket of `kind: deps` or `interface` may do (M1-F1 is `kind: feature`)";
    let with_a_profile = format!("{MANIFEST}\n[profile.release]\nlto = \"fat\"\n");
    let with_a_feature = MANIFEST.replace("[\"derive\"]", "[\"derive\", \"rc\"]");
    let with_a_table =
        format!("{MANIFEST}\n[workspace.dependencies.sysinfo]\nversion = \"=0.31.4\"\n");
    let hiding_one = MANIFEST.replace(
        "anyhow = { version = \"=1.0.104\" }",
        "anyhow = { version = \"=1.0.104\", features = [\n] }\nsysinfo = \"=0.31.4\"",
    );

    let elsewhere = fixture.head(
        &base,
        &[
            ("Cargo.toml", Some(&with_a_profile)),
            (
                "crates/a/Cargo.toml",
                Some("[package]\nname = \"a\"\nedition = \"2024\"\n"),
            ),
            ("crates/a/Cargo.lock", Some("# Not the lockfile.\n")),
        ],
        "M1-F1: a profile",
    );
    assert_eq!(
        failed(&fixture.judged("m1/f1-model", &base, &elsewhere)),
        NONE
    );

    let unreadable = "Cargo.toml is changed and its dependencies cannot be read (line 7 is not a table name or one `key = value` that ends on its line)";
    let cases: [(&str, Option<&str>, &str); 6] = [
        (
            "Cargo.lock",
            Some("# The lockfile.\nversion = 4\n# changed\n"),
            "Cargo.lock is changed",
        ),
        ("Cargo.lock", None, "Cargo.lock is changed"),
        (
            "deny.toml",
            Some("[bans]\ndeny = []\nskip = []\n"),
            "deny.toml is changed",
        ),
        (
            "Cargo.toml",
            Some(&with_a_feature),
            "[workspace.dependencies] of Cargo.toml is changed",
        ),
        (
            "Cargo.toml",
            Some(&with_a_table),
            "[workspace.dependencies] of Cargo.toml is changed",
        ),
        ("Cargo.toml", Some(&hiding_one), unreadable),
    ];
    for (path, text, problem) in cases {
        let head = fixture.head(&base, &[(path, text)], "M1-F1: a dependency");
        let report = fixture.judged("m1/f1-model", &base, &head);
        assert_eq!(failed(&report), [Rule::Dependencies], "{problem}");
        assert_eq!(
            problems(&report, Rule::Dependencies),
            [format!("{problem}{only}")]
        );
        for branch in ["m1/d1-deps", "m1/i1-interface"] {
            assert_eq!(
                failed(&fixture.judged(branch, &base, &head)),
                NONE,
                "{branch}"
            );
        }
    }

    let respelled = fixture.staged(
        &base,
        &[("100644", "cargo.lock")],
        "M1-F1: a second lockfile",
    );
    assert_eq!(
        problems(
            &fixture.judged("m1/f1-model", &base, &respelled),
            Rule::Dependencies
        ),
        [format!("Cargo.lock is changed{only}")]
    );
    let as_a_link = fixture.staged(
        &base,
        &[("120000", "Cargo.toml")],
        "M1-F1: the manifest as a link",
    );
    assert_eq!(
        problems(
            &fixture.judged("m1/f1-model", &base, &as_a_link),
            Rule::Dependencies
        ),
        [format!(
            "Cargo.toml is changed and its dependencies cannot be read (it is not a regular file){only}"
        )]
    );

    fixture.on("m1/f1-model", &base);
    assert_eq!(
        failed(&checkout(&fixture.repo, &base, "M1-F1").unwrap()),
        NONE
    );
    fixture.write("Cargo.toml", &with_a_profile);
    assert_eq!(
        failed(&checkout(&fixture.repo, &base, "M1-F1").unwrap()),
        NONE
    );
    fixture.write("Cargo.toml", &with_a_feature);
    let report = checkout(&fixture.repo, &base, "M1-F1").unwrap();
    assert_eq!(failed(&report), [Rule::Dependencies]);
    assert_eq!(
        problems(&report, Rule::Dependencies),
        [format!(
            "[workspace.dependencies] of Cargo.toml is changed{only}"
        )]
    );
}

#[test]
fn rule_8_the_body_has_its_three_sections_and_an_evidence_block_that_parses() {
    let (fixture, base) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        ("plan/tickets/M1-F1.md", &ticket("feature", &["crates/**"])),
        ("crates/a/src/lib.rs", "pub fn one() {}\n"),
    ]);
    let head = fixture.head(
        &base,
        &[("crates/a/src/lib.rs", Some("pub fn two() {}\n"))],
        "M1-F1: a change",
    );
    let with_body = |body: &str| {
        let mut request = fixture.request("m1/f1-model", &base, &head);
        request.body = body.to_owned();
        pull_request(&fixture.repo, &base, &request, None).unwrap()
    };

    for body in [
        BODY.to_owned(),
        BODY.replace("```evidence\n```", "```evidence\n{}\n```"),
        BODY.replace('\n', "\r\n"),
    ] {
        assert_eq!(failed(&with_body(&body)), NONE, "{body}");
    }
    for (body, problem) in [
        (
            BODY.replace("```evidence\n```", "```evidence\n[]\n```"),
            "the evidence block that opens on line 7 of the body is JSON but not an object",
        ),
        (
            BODY.replace("```evidence\n```", "```json\n{}\n```"),
            "the Evidence section has no ```evidence block (an empty one is valid)",
        ),
        (
            BODY.replace("```evidence\n```\n", ""),
            "the Evidence section has no ```evidence block (an empty one is valid)",
        ),
        (
            BODY.replace("## Merge Danger", "## Risks"),
            "the body has no `## Merge Danger` section",
        ),
        (
            format!("```\n{BODY}"),
            "the body has no `## Summary` section",
        ),
    ] {
        let report = with_body(&body);
        assert_eq!(failed(&report), [Rule::Evidence], "{body}");
        assert_eq!(problems(&report, Rule::Evidence)[0], problem);
    }
    assert_eq!(problems(&with_body(""), Rule::Evidence).len(), 3);
}

#[test]
fn sixteen_names_that_are_not_utf8_fail_the_run_and_do_not_count_as_one_file() {
    let (fixture, base) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        ("plan/tickets/M1-F1.md", &ticket("feature", &["crates/**"])),
    ]);
    let names: Vec<Vec<u8>> = (0x80..=0x8f_u8)
        .map(|byte| [b"f".as_slice(), &[byte], b".rs"].concat())
        .collect();
    let names: Vec<&[u8]> = names.iter().map(Vec::as_slice).collect();
    let head = fixture.head_with_names(&base, Some("crates"), &names);
    let request = fixture.request("m1/f1-model", &base, &head);
    let error = pull_request(&fixture.repo, &base, &request, None)
        .err()
        .expect("sixteen files whose names are not UTF-8");
    assert_eq!(
        error.to_string(),
        "git printed a path that is not UTF-8, which this check cannot judge"
    );

    let readable: Vec<Vec<u8>> = (0..16)
        .map(|index| format!("f{index}.rs").into_bytes())
        .collect();
    let readable: Vec<&[u8]> = readable.iter().map(Vec::as_slice).collect();
    let head = fixture.head_with_names(&base, Some("crates"), &readable);
    let report = fixture.judged("m1/f1-model", &base, &head);
    assert_eq!(failed(&report), [Rule::Scope]);
}

fn event_file(fixture: &Fixture, text: &str) -> String {
    let path = fixture.path(".git/event.json");
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn the_workflow_s_command_line_judges_a_pull_request_and_a_push() {
    let (fixture, base) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        (
            "plan/tickets/M1-F1.md",
            &ticket("feature", &["crates/**", "plan/**"]),
        ),
        ("plan/triage.toml", &row("S1-01", "", "keep")),
        ("crates/a/src/lib.rs", "pub fn one() {}\n"),
    ]);
    let head = fixture.head(
        &base,
        &[("crates/a/src/lib.rs", Some("pub fn two() {}\n"))],
        "M1-F1: a change",
    );
    let owners = fixture.head(
        &base,
        &[("plan/triage.toml", Some(&row("S1-01", "", "drop")))],
        "M1-F1: one of the owner's rows",
    );
    let event = |head: &str, body: &str| {
        format!(
            r#"{{"action": "opened", "number": 9, "pull_request": {{"number": 9, "title": "M1-F1: a change", "body": {},
            "base": {{"ref": "main", "sha": "{base}"}}, "head": {{"ref": "m1/f1-model", "sha": "{head}"}}}},
            "repository": {{"name": "fixture", "default_branch": "main", "owner": {{"login": "someone", "id": {OWNER}}}}}}}"#,
            serde_json::to_string(body).unwrap()
        )
    };
    let reviews_file = fixture.path(".git/reviews.json");
    let reviews_path = reviews_file.to_string_lossy().into_owned();

    let path = event_file(&fixture, &event(&head, BODY));
    let report = on_event(&fixture.repo, &args(&["--base", &base, "--event", &path])).unwrap();
    assert_eq!(failed(&report), NONE);
    assert_eq!(judged_rules(&report).len(), 8);

    std::fs::write(&reviews_file, "[]").unwrap();
    let with_reviews = args(&[
        "--base",
        &base,
        "--event",
        &path,
        "--reviews",
        &reviews_path,
    ]);
    assert_eq!(
        failed(&on_event(&fixture.repo, &with_reviews).unwrap()),
        NONE
    );

    let path = event_file(&fixture, &event(&head, "No sections."));
    let report = on_event(&fixture.repo, &with_reviews).unwrap();
    assert_eq!(failed(&report), [Rule::Evidence]);

    event_file(&fixture, &event(&owners, BODY));
    assert_eq!(
        failed(&on_event(&fixture.repo, &with_reviews).unwrap()),
        [Rule::OwnersRows]
    );
    let approval = format!(
        r#"[{{"id": 1, "user": {{"login": "someone", "id": {OWNER}}}, "state": "APPROVED", "commit_id": "{owners}", "submitted_at": "2026-11-14T10:00:00Z"}}]"#
    );
    std::fs::write(&reviews_file, approval).unwrap();
    assert_eq!(
        failed(&on_event(&fixture.repo, &with_reviews).unwrap()),
        NONE
    );

    std::fs::remove_file(&reviews_file).unwrap();
    let missing = on_event(&fixture.repo, &with_reviews)
        .err()
        .expect("no reviews file");
    assert!(
        missing.to_string().starts_with("could not read "),
        "{missing}"
    );

    event_file(&fixture, &event(&head, BODY));
    let moved_on = fixture.head(
        &base,
        &[("plan/PLAN.md", Some("Merged since the event.\n"))],
        "Plan: merged since the event",
    );
    let later_base = args(&["--base", &moved_on, "--event", &path]);
    let report = on_event(&fixture.repo, &later_base).unwrap();
    assert_eq!(failed(&report), NONE);
    assert!(
        report.about.ends_with("1 changed paths, 1 commits"),
        "{}",
        report.about
    );
    let holding_the_head = args(&["--base", &head, "--event", &path]);
    let error = on_event(&fixture.repo, &holding_the_head)
        .err()
        .expect("a base that holds the head");
    assert_eq!(
        error.to_string(),
        "the head commit is already in the history of --base, so there is nothing to judge"
    );
    event_file(&fixture, &event(&head, BODY).replace(&base, &moved_on));
    let error = on_event(&fixture.repo, &args(&["--base", &base, "--event", &path]))
        .err()
        .expect("a base older than the event's");
    assert_eq!(
        error.to_string(),
        "the base commit the event names is not in the history of --base"
    );
    event_file(&fixture, &event(&head, BODY));

    let credited = fixture.head(
        &base,
        &[("crates/a/src/lib.rs", Some("pub fn two() {}\n"))],
        &format!("M1-F1: a change (#9)\n\n{TRAILER}"),
    );
    let pushed = |after: &str| {
        format!(
            r#"{{"ref": "refs/heads/main", "before": "{base}", "after": "{after}", "commits": []}}"#
        )
    };
    event_file(&fixture, &pushed(&credited));
    let report = on_event(&fixture.repo, &with_reviews).unwrap();
    assert_eq!(judged_rules(&report), [Rule::Attribution]);
    assert_eq!(failed(&report), [Rule::Attribution]);
    event_file(&fixture, &pushed(&head));
    assert_eq!(
        failed(&on_event(&fixture.repo, &with_reviews).unwrap()),
        NONE
    );

    event_file(
        &fixture,
        &event(&head, BODY).replace("\"ref\": \"main\"", "\"ref\": \"side\""),
    );
    let error = on_event(&fixture.repo, &with_reviews)
        .err()
        .expect("another target");
    assert_eq!(
        error.to_string(),
        "the pull request does not target the default branch, so a pass here would say nothing about a merge there"
    );

    event_file(&fixture, r#"{"action": "created", "issue": {}}"#);
    let error = on_event(&fixture.repo, &with_reviews)
        .err()
        .expect("another event");
    assert_eq!(
        error.to_string(),
        "the event is neither a pull request nor a push"
    );
    for wrong in [
        args(&[]),
        args(&["--base", &base]),
        args(&["--event", &path]),
        args(&["--base", &base, "--event"]),
        args(&["--base", &base, "--event", &path, "--head", &head]),
        args(&["--base", &base, "--base", &base, "--event", &path]),
    ] {
        let error = on_event(&fixture.repo, &wrong)
            .err()
            .expect("a usage error");
        assert!(error.to_string().starts_with("usage: "), "{wrong:?}");
    }
}

#[test]
fn check_ticket_judges_the_checkout_of_a_branch_with_no_pull_request() {
    let (fixture, base) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        (".gitignore", "target/\n*.local.*\n"),
        (
            "plan/tickets/M1-F1.md",
            &ticket("feature", &["crates/ol-model/**"]),
        ),
        ("crates/ol-model/src/lib.rs", "pub fn one() {}\n"),
        ("crates/ol-model/src/old.rs", "pub fn old() {}\n"),
        ("docs/guide.md", "A guide.\n"),
    ]);
    let judged = || {
        on_checkout(
            &fixture.repo,
            &args(&["--ticket", "M1-F1", "--base", &base]),
        )
        .unwrap()
    };
    let outside = |path: &str| vec![format!("outside the write_scope of M1-F1: {path}")];

    fixture.on("m1/f1-model", &base);
    let report = judged();
    assert_eq!(failed(&report), NONE);
    assert_eq!(
        judged_rules(&report),
        [
            Rule::Ticket,
            Rule::Scope,
            Rule::Immutable,
            Rule::Apart,
            Rule::Attribution,
            Rule::Dependencies
        ]
    );

    fixture.write("crates/ol-model/src/lib.rs", "pub fn two() {}\n");
    fixture.commit("M1-F1: committed");
    fixture.write("crates/ol-model/src/staged.rs", "pub fn staged() {}\n");
    fixture.git(&["add", "crates/ol-model/src/staged.rs"]);
    fixture.write(
        "crates/ol-model/src/old.rs",
        "pub fn old() { /* not staged */ }\n",
    );
    fixture.write(
        "crates/ol-model/src/untracked.rs",
        "pub fn untracked() {}\n",
    );
    fixture.write("target/debug/build.log", "ignored\n");
    fixture.write(".claude/ticket.local.json", "{}\n");
    let report = judged();
    assert_eq!(failed(&report), NONE);
    assert!(
        report.about.ends_with("4 changed paths, 1 commits"),
        "{}",
        report.about
    );

    fixture.write("docs/untracked.md", "Not in the scope.\n");
    assert_eq!(
        problems(&judged(), Rule::Scope),
        outside("docs/untracked.md")
    );
    fixture.git(&["add", "docs/untracked.md"]);
    assert_eq!(
        problems(&judged(), Rule::Scope),
        outside("docs/untracked.md")
    );
    fixture.remove("docs/untracked.md");
    assert_eq!(
        problems(&judged(), Rule::Scope),
        outside("docs/untracked.md")
    );
    fixture.git(&["rm", "-q", "--cached", "docs/untracked.md"]);
    assert_eq!(failed(&judged()), NONE);

    fixture.write("docs/guide.md", "A guide, changed but not staged.\n");
    assert_eq!(problems(&judged(), Rule::Scope), outside("docs/guide.md"));
    fixture.git(&["add", "docs/guide.md"]);
    fixture.write("docs/guide.md", "A guide.\n");
    assert_eq!(problems(&judged(), Rule::Scope), outside("docs/guide.md"));
    fixture.git(&["reset", "-q", "--", "docs/guide.md"]);
    assert_eq!(failed(&judged()), NONE);

    fixture.remove("docs/guide.md");
    assert_eq!(problems(&judged(), Rule::Scope), outside("docs/guide.md"));
    fixture.write("docs/guide.md", "A guide.\n");
    fixture.write("docs/committed.md", "Not in the scope.\n");
    fixture.commit("M1-F1: one file outside the scope");
    assert_eq!(
        problems(&judged(), Rule::Scope),
        outside("docs/committed.md")
    );
    fixture.git(&["rm", "-q", "--cached", "docs/committed.md"]);
    fixture.remove("docs/committed.md");
    assert_eq!(
        problems(&judged(), Rule::Scope),
        outside("docs/committed.md")
    );
}

#[test]
fn check_ticket_needs_a_branch_that_names_the_ticket_given() {
    let (fixture, base) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        ("plan/tickets/M1-F1.md", &ticket("feature", &["crates/**"])),
        ("plan/tickets/M1-F2.md", &ticket("feature", &["crates/**"])),
    ]);
    let ticket_problems = |id: &str| {
        let report = on_checkout(&fixture.repo, &args(&["--ticket", id, "--base", &base])).unwrap();
        problems(&report, Rule::Ticket)
    };
    let none: [&str; 0] = [];

    fixture.on("m1/f1-model", &base);
    assert_eq!(ticket_problems("M1-F1"), none);
    assert_eq!(ticket_problems("m1-f1"), none);
    assert_eq!(
        ticket_problems("M1-F2"),
        ["the branch does not name M1-F2 (`<milestone>/<ticket>-...`)"]
    );
    assert_eq!(
        ticket_problems("M9-Z9"),
        [
            "M9-Z9 has no file in plan/tickets/ on the base",
            "the branch does not name M9-Z9 (`<milestone>/<ticket>-...`)"
        ]
    );
    fixture.on("main", &base);
    assert_eq!(
        ticket_problems("M1-F1"),
        ["the branch does not name M1-F1 (`<milestone>/<ticket>-...`)"]
    );
    fixture.git(&["checkout", "-q", "--detach", &base]);
    assert_eq!(
        ticket_problems("M1-F1"),
        ["HEAD is on no branch, so no branch names M1-F1"]
    );

    for wrong in [
        args(&[]),
        args(&["--ticket"]),
        args(&["--ticket", "TEMPLATE"]),
        args(&["--ticket", "../M1-F1"]),
        args(&["--ticket", "M1-F1", "--tier", "0"]),
        args(&["--base", &base]),
    ] {
        let error = on_checkout(&fixture.repo, &wrong)
            .err()
            .expect("a usage error");
        assert!(error.to_string().starts_with("usage: "), "{wrong:?}");
    }
    let no_remote = on_checkout(&fixture.repo, &args(&["--ticket", "M1-F1"]));
    let error = no_remote.err().expect("no origin/main in a fixture");
    assert!(
        error
            .to_string()
            .starts_with("refs/remotes/origin/main is not a commit in this checkout"),
        "{error}"
    );
}

#[test]
fn check_ticket_passes_an_interface_ticket_of_many_files_and_fails_one_file_outside_it() {
    let scope = [
        "Cargo.toml",
        "Cargo.lock",
        "deny.toml",
        "crates/**",
        "xtask/**",
    ];
    let (fixture, base) = Fixture::new(&[
        ("README.md", "A fixture.\n"),
        ("plan/tickets/M1-F1.md", &ticket("interface", &scope)),
        ("plan/tickets/M1-F2.md", &ticket("feature", &scope)),
    ]);
    fixture.on("m1/f1-workspace", &base);
    fixture.write("Cargo.toml", MANIFEST);
    fixture.write("Cargo.lock", "# The lockfile.\nversion = 4\n");
    fixture.write("deny.toml", "[bans]\ndeny = []\n");
    for index in 0..20 {
        fixture.write(
            &format!("crates/ol-c{index}/src/lib.rs"),
            "#![forbid(unsafe_code)]\n",
        );
    }
    fixture.commit("M1-F1: the workspace");
    let judged =
        |id: &str| on_checkout(&fixture.repo, &args(&["--ticket", id, "--base", &base])).unwrap();
    assert_eq!(failed(&judged("M1-F1")), NONE);

    fixture.write("docs/guide.md", "One file outside the scope.\n");
    let report = judged("M1-F1");
    assert_eq!(failed(&report), [Rule::Scope]);
    assert_eq!(
        problems(&report, Rule::Scope),
        ["outside the write_scope of M1-F1: docs/guide.md"]
    );
    fixture.remove("docs/guide.md");

    fixture.on("m1/f2-workspace", "m1/f1-workspace");
    assert_eq!(failed(&judged("M1-F2")), [Rule::Scope, Rule::Dependencies]);
}
