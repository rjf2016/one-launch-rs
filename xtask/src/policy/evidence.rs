use std::sync::LazyLock;

use regex::Regex;

pub const SECTIONS: [&str; 3] = ["Summary", "Evidence", "Merge Danger"];
const BLOCK: &str = "evidence";
const BLANK: [char; 2] = [' ', '\t'];
const INDENTED_FENCE: &str = "a code fence that is not at the margin";
const FENCE_IN_HTML: &str = "a code fence under a line of raw HTML with no blank line between";
const INDENTED_RAW: &str = "a comment or a raw block that is not at the margin";
const RAW_IN_HTML: &str =
    "a comment or a raw block under a line of raw HTML with no blank line between";
const OPEN_COMMENT: &str =
    "an HTML comment that opens in the middle of a line and does not close on it";

static HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(#{1,6})[ \t]+(.*?)(?:[ \t]+#+)?[ \t]*$").expect("a fixed pattern")
});
static TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^</?[A-Za-z]").expect("a fixed pattern"));
static RAW: LazyLock<Vec<(Regex, &'static [&'static str])>> = LazyLock::new(|| {
    let blocks: [(&str, &'static [&'static str]); 5] = [
        (
            r"(?i)^<(?:pre|script|style|textarea)(?:[ \t>]|$)",
            &["</pre>", "</script>", "</style>", "</textarea>"],
        ),
        (r"^<!--", &["-->"]),
        (r"^<\?", &["?>"]),
        (r"^<!\[CDATA\[", &["]]>"]),
        (r"^<![A-Za-z]", &[">"]),
    ];
    blocks
        .into_iter()
        .map(|(opens, ends)| (Regex::new(opens).expect("a fixed pattern"), ends))
        .collect()
});

#[derive(Debug, PartialEq)]
enum Line<'a> {
    Heading(usize, &'a str),
    Opens(&'a str),
    Code(&'a str),
    Closes,
    Other,
    Unclear(&'static str),
}

enum Within {
    Text,
    Fence(char, usize),
    Raw(&'static [&'static str]),
    Html,
}

fn fence(text: &str) -> Option<(char, usize, &str)> {
    let mark = text.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let length = text.chars().take_while(|c| *c == mark).count();
    let info = &text[length..];
    (length >= 3 && (mark == '~' || !info.contains('`'))).then_some((mark, length, info))
}

fn raw(text: &str) -> Option<(&str, &'static [&'static str])> {
    RAW.iter()
        .find_map(|(opens, ends)| Some((&text[opens.find(text)?.end()..], *ends)))
}

fn after<'a>(text: &'a str, ends: &[&str]) -> Option<&'a str> {
    let lower = text.to_ascii_lowercase();
    ends.iter()
        .filter_map(|end| Some(lower.find(end)? + end.len()))
        .min()
        .map(|at| &text[at..])
}

fn opens_a_comment(mut text: &str) -> bool {
    while let Some(at) = text.find("<!--") {
        let inside = &text[at + 4..];
        match inside.find("-->") {
            Some(end) => text = &inside[end + 3..],
            None => return true,
        }
    }
    false
}

// No Markdown parser is approved, so the body is read only where a line can be placed for certain: code fences at the margin, comments and raw blocks at the margin, and raw HTML that a blank line ends. Whatever could be read two ways (a fence in a list, a fence under a tag) is named as unclear and fails, because past it this reading and GitHub's could disagree about what is code.
fn read(body: &str) -> Vec<Line<'_>> {
    let mut within = Within::Text;
    body.lines()
        .map(|line| {
            let text = line.trim_start_matches(BLANK);
            let indented = text.len() != line.len();
            match within {
                Within::Fence(mark, length) => {
                    let end = line.trim_end_matches(BLANK);
                    let run = end.trim_start_matches(' ');
                    let closes = end.len() - run.len() <= 3
                        && run.len() >= length
                        && run.chars().all(|c| c == mark);
                    if closes {
                        within = Within::Text;
                        return Line::Closes;
                    }
                    Line::Code(line)
                }
                Within::Raw(ends) => match after(line, ends) {
                    None => Line::Other,
                    Some(rest) => {
                        within = Within::Text;
                        if opens_a_comment(rest) {
                            return Line::Unclear(OPEN_COMMENT);
                        }
                        Line::Other
                    }
                },
                Within::Html => {
                    if text.is_empty() {
                        within = Within::Text;
                        Line::Other
                    } else if fence(text).is_some() {
                        Line::Unclear(FENCE_IN_HTML)
                    } else if raw(text).is_some() {
                        Line::Unclear(RAW_IN_HTML)
                    } else if opens_a_comment(line) {
                        Line::Unclear(OPEN_COMMENT)
                    } else {
                        Line::Other
                    }
                }
                Within::Text => {
                    if let Some((mark, length, info)) = fence(text) {
                        if indented {
                            return Line::Unclear(INDENTED_FENCE);
                        }
                        within = Within::Fence(mark, length);
                        return Line::Opens(info.trim_matches(BLANK));
                    }
                    if let Some((rest, ends)) = raw(text) {
                        if indented {
                            return Line::Unclear(INDENTED_RAW);
                        }
                        match after(rest, ends) {
                            None => within = Within::Raw(ends),
                            Some(rest) if opens_a_comment(rest) => {
                                return Line::Unclear(OPEN_COMMENT);
                            }
                            Some(_) => {}
                        }
                        return Line::Other;
                    }
                    if opens_a_comment(line) {
                        return Line::Unclear(OPEN_COMMENT);
                    }
                    if TAG.is_match(text) {
                        within = Within::Html;
                        return Line::Other;
                    }
                    match HEADING.captures(line) {
                        Some(found) => Line::Heading(
                            found[1].len(),
                            found.get(2).map_or("", |title| title.as_str()),
                        ),
                        None => Line::Other,
                    }
                }
            }
        })
        .collect()
}

pub fn check(body: &str) -> Vec<String> {
    let body = body.replace("\r\n", "\n").replace('\r', "\n");
    let lines = read(&body);
    let mut problems: Vec<String> = lines
        .iter()
        .enumerate()
        .filter_map(|(at, line)| match line {
            Line::Unclear(what) => Some(format!(
                "line {} of the body: {what}, so this check cannot tell how GitHub reads what follows",
                at + 1
            )),
            _ => None,
        })
        .collect();
    let mut once = Vec::new();
    for name in SECTIONS {
        let places: Vec<usize> = (0..lines.len())
            .filter(|at| lines[*at] == Line::Heading(2, name))
            .collect();
        match places.as_slice() {
            [place] => once.push((name, *place)),
            [] => problems.push(format!("the body has no `## {name}` section")),
            many => problems.push(format!("the body has {} `## {name}` sections", many.len())),
        }
    }
    if once.len() == SECTIONS.len() && !once.is_sorted_by_key(|(_, place)| *place) {
        problems.push(format!(
            "the sections are not in the order {}",
            SECTIONS.join(", ")
        ));
    }
    if let Some((_, heading)) = once.iter().find(|(name, _)| *name == "Evidence") {
        let from = heading + 1;
        let to = lines[from..]
            .iter()
            .position(|line| matches!(line, Line::Heading(level, _) if *level <= 2))
            .map_or(lines.len(), |next| from + next);
        let blocks: Vec<usize> = (from..to)
            .filter(|at| lines[*at] == Line::Opens(BLOCK))
            .collect();
        match blocks.as_slice() {
            [block] => problems.extend(unparsed(&lines, *block)),
            [] => problems.push(format!(
                "the Evidence section has no ```{BLOCK} block (an empty one is valid)"
            )),
            many => problems.push(format!(
                "the Evidence section has {} ```{BLOCK} blocks",
                many.len()
            )),
        }
    }
    problems
}

// The block's text is the author's: a problem names where it is, never what it holds.
fn unparsed(lines: &[Line], opens: usize) -> Option<String> {
    let code: Vec<&str> = lines[opens + 1..]
        .iter()
        .map_while(|line| match line {
            Line::Code(text) => Some(*text),
            _ => None,
        })
        .collect();
    let block = format!(
        "the evidence block that opens on line {} of the body",
        opens + 1
    );
    if lines.get(opens + 1 + code.len()) != Some(&Line::Closes) {
        return Some(format!("{block} is never closed"));
    }
    let text = code.join("\n");
    if text.trim().is_empty() {
        return None;
    }
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(value) if value.is_object() => None,
        Ok(_) => Some(format!("{block} is JSON but not an object")),
        Err(error) => Some(format!(
            "{block} is not JSON (line {}, column {} of the block)",
            error.line(),
            error.column()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(evidence: &str) -> String {
        format!(
            "## Summary\n\nWhat changed.\n\n## Evidence\n\n{evidence}\n\n## Merge Danger\n\nNone.\n"
        )
    }

    fn unclear(line: usize, what: &str) -> String {
        format!(
            "line {line} of the body: {what}, so this check cannot tell how GitHub reads what follows"
        )
    }

    const EMPTY: &str = "```evidence\n```";
    const ALL: &str = "## Summary\n\n## Evidence\n\n```evidence\n```\n\n## Merge Danger\n";
    const NO_SUMMARY: &str = "the body has no `## Summary` section";

    #[test]
    fn the_three_sections_and_an_empty_block_pass() {
        for text in [
            body(EMPTY),
            body("```evidence\n\n   \n```"),
            body("```evidence\n{}\n```"),
            body(
                "```evidence\n{\n  \"tests\": { \"added\": [\"a\"], \"removed\": [] },\n  \"later\": 1\n}\n```",
            ),
            body(
                "Checks:\n\n```text\n$ cargo xtask check\n== check passed\n```\n\n````evidence\n{}\n````\n\n### Notes\n\nMore.",
            ),
            body("~~~ evidence\n{}\n~~~"),
            body("```evidence  \n{}\n   ```\t"),
            body(EMPTY).replace('\n', "\r\n"),
            body(EMPTY).replace('\n', "\r"),
            body(EMPTY).replace("## Summary", "# A title\n\n## Summary ##"),
            format!(
                "{}\n## Notes\n\n```evidence\nnot read here\n```\n",
                body(EMPTY)
            ),
            format!("<!-- a note -->\n```\ncode\n```\n<details>\n<summary>x</summary>\n\n{ALL}"),
            format!("- a list\n  <br>\n\n1. one\n\n> ```\n> quoted\n> ```\n\n{ALL}"),
            format!("Inline ```code``` and <https://example.com> and a < b.\n{ALL}"),
            format!("```code``` at the start of a line is not a fence.\n\n{ALL}"),
        ] {
            assert_eq!(check(&text), Vec::<String>::new(), "{text}");
        }
    }

    #[test]
    fn a_missing_a_doubled_or_a_misplaced_section_fails() {
        assert_eq!(
            check(""),
            [
                NO_SUMMARY,
                "the body has no `## Evidence` section",
                "the body has no `## Merge Danger` section"
            ]
        );
        assert_eq!(
            check(&body(EMPTY).replace("## Merge Danger", "## Merge danger")),
            ["the body has no `## Merge Danger` section"]
        );
        for (from, to) in [
            ("## Summary", "### Summary"),
            ("## Summary", "# Summary"),
            ("## Summary", " ## Summary"),
            ("## Summary", "##Summary"),
            ("## Summary", "> ## Summary"),
            ("## Summary", "- ## Summary"),
            ("## Summary", "\\## Summary"),
            ("## Summary", "## Summary of the change"),
            ("## Summary", "Summary\n-------"),
        ] {
            assert_eq!(check(&body(EMPTY).replace(from, to)), [NO_SUMMARY], "{to}");
        }
        assert_eq!(
            check(&format!("{}\n## Summary\n", body(EMPTY))),
            ["the body has 2 `## Summary` sections"]
        );
        assert_eq!(
            check("## Evidence\n\n```evidence\n```\n\n## Summary\n\n## Merge Danger\n"),
            ["the sections are not in the order Summary, Evidence, Merge Danger"]
        );
    }

    #[test]
    fn a_heading_inside_code_or_a_raw_block_is_not_a_section() {
        let close_together = ALL.replace("\n\n", "\n");
        for hidden in [
            format!("```\n{ALL}```\n"),
            format!("```markdown\n{ALL}\n"),
            format!("~~~~\n{ALL}~~~\n"),
            format!("~~~ a `tilde` fence\n{ALL}~~~\n"),
            format!("````\n{ALL}```\n"),
            format!("```\ncode\n    ```\n{ALL}"),
            format!("```\ncode\n```\u{a0}\n{ALL}"),
            format!("```\ncode\n``` x\n{ALL}"),
            format!("text\r```\n{ALL}"),
            format!("<!--\n{ALL}-->\n"),
            format!("<!-- a note\n\n{ALL}\n--> \n"),
            format!("<pre>\n\n{ALL}\n</pre>\n"),
            format!("<PRE lang=x>\n\n{ALL}\n</Pre>\n"),
            format!("<script>\n\n{ALL}\n</style>\n"),
            format!("<?php\n\n{ALL}\n?>\n"),
            format!("<![CDATA[\n\n{ALL}\n]]>\n"),
            format!("<!DOCTYPE html\n\n{ALL}\n>\n"),
            format!("<details>\n{close_together}</details>\n"),
            format!("<div hidden>\n{close_together}</div>\n"),
            format!("<div>\n\u{a0}\n{close_together}"),
            format!("  <div>\n{close_together}"),
            ALL.replace("##", "    ##"),
        ] {
            let problems = check(&hidden);
            assert!(
                problems.contains(&NO_SUMMARY.to_owned()),
                "{hidden}: {problems:?}"
            );
        }
    }

    #[test]
    fn what_could_be_read_two_ways_fails_as_unclear() {
        let tilde = ALL.replace("```", "~~~");
        for (text, line, what) in [
            (format!("<span>x\n```\n\n{tilde}"), 2, FENCE_IN_HTML),
            (format!("<span>x\n   ~~~\n\n{ALL}"), 2, FENCE_IN_HTML),
            (format!("1. x\n   ```\n```\n{ALL}"), 2, INDENTED_FENCE),
            (format!("- x\n\n      ```\n{ALL}"), 3, INDENTED_FENCE),
            (format!("\t```\n{ALL}"), 1, INDENTED_FENCE),
            (format!(" <!--\n{ALL}"), 1, INDENTED_RAW),
            (format!("- x\n  <pre>\n{ALL}"), 2, INDENTED_RAW),
            (format!("<p>x\n<!--\n\n{ALL}\n-->\n"), 2, RAW_IN_HTML),
            (format!("<p>x\n<pre>\n\n{ALL}"), 2, RAW_IN_HTML),
            (format!("<p><!--\n\n{ALL}"), 1, OPEN_COMMENT),
            (format!("see <!-- a note\n{ALL}"), 1, OPEN_COMMENT),
            (format!("<!-- a --> <!-- b\n{ALL}"), 1, OPEN_COMMENT),
            (format!("<!--\na\n--> x <!-- b\n{ALL}"), 3, OPEN_COMMENT),
            (format!("<p>x\ny <!-- z\n\n{ALL}"), 2, OPEN_COMMENT),
        ] {
            let problems = check(&text);
            assert_eq!(problems.first(), Some(&unclear(line, what)), "{text}");
        }
        let in_the_evidence = check(&body("    ```evidence\n    ```"));
        assert_eq!(
            in_the_evidence,
            [
                unclear(7, INDENTED_FENCE),
                unclear(8, INDENTED_FENCE),
                "the Evidence section has no ```evidence block (an empty one is valid)".to_owned()
            ]
        );
    }

    #[test]
    fn the_evidence_block_must_be_there_once_closed_and_a_json_object() {
        let block = "the evidence block that opens on line 7 of the body";
        for (evidence, problem) in [
            (
                "Nothing to show.".to_owned(),
                "the Evidence section has no ```evidence block (an empty one is valid)".to_owned(),
            ),
            (
                "```json\n{}\n```".to_owned(),
                "the Evidence section has no ```evidence block (an empty one is valid)".to_owned(),
            ),
            (
                "```Evidence\n```".to_owned(),
                "the Evidence section has no ```evidence block (an empty one is valid)".to_owned(),
            ),
            (
                "```evidence json\n```".to_owned(),
                "the Evidence section has no ```evidence block (an empty one is valid)".to_owned(),
            ),
            (
                format!("{EMPTY}\n\n{EMPTY}"),
                "the Evidence section has 2 ```evidence blocks".to_owned(),
            ),
            (
                "```evidence\n[]\n```".to_owned(),
                format!("{block} is JSON but not an object"),
            ),
            (
                "```evidence\n\"{}\"\n```".to_owned(),
                format!("{block} is JSON but not an object"),
            ),
            (
                "```evidence\n{\n  \"tests\": [1,\n}\n```".to_owned(),
                format!("{block} is not JSON (line 3, column 1 of the block)"),
            ),
            (
                "```evidence\n{} {}\n```".to_owned(),
                format!("{block} is not JSON (line 1, column 4 of the block)"),
            ),
        ] {
            assert_eq!(check(&body(&evidence)), [problem], "{evidence}");
        }
        assert_eq!(
            check("## Summary\n\n## Merge Danger\n\n## Evidence\n\n```evidence\n{}\n"),
            [
                "the sections are not in the order Summary, Evidence, Merge Danger".to_owned(),
                "the evidence block that opens on line 7 of the body is never closed".to_owned()
            ]
        );
        assert_eq!(
            check(&body("````evidence\n{}\n```")),
            [
                "the body has no `## Merge Danger` section".to_owned(),
                format!("{block} is never closed")
            ]
        );
    }

    #[test]
    fn a_problem_never_quotes_the_block() {
        let held = "held-in-the-block";
        for evidence in [
            format!("```evidence\n\"{held}\"\n```"),
            format!("```evidence\n{{ \"{held}\": {held} }}\n```"),
            format!("```evidence\n{held}\n```"),
        ] {
            let problems = check(&body(&evidence));
            assert_eq!(problems.len(), 1, "{evidence}");
            assert!(!problems[0].contains(held), "{problems:?}");
        }
    }
}
