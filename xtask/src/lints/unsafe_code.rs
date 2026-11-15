use anyhow::{Context, Result};

use crate::workspace::{self, Workspace};

const ATTRIBUTE: &str = "#![forbid(unsafe_code)]";
const EXEMPT: &str = "ol-platform-win";

pub fn check(workspace: &Workspace) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    for member in &workspace.members {
        if member.is_spike() || member.name == EXEMPT {
            continue;
        }
        for root in &member.roots {
            let source = std::fs::read_to_string(root)
                .with_context(|| format!("could not read {}", root.display()))?;
            if !starts_with_the_attribute(&source) {
                let shown = root.strip_prefix(workspace::root()).unwrap_or(root);
                problems.push(format!(
                    "{}: the first line of code must be {ATTRIBUTE}",
                    shown.display()
                ));
            }
        }
    }
    Ok(problems)
}

fn starts_with_the_attribute(source: &str) -> bool {
    source
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("//"))
        == Some(ATTRIBUTE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_attribute_first_passes() {
        assert!(starts_with_the_attribute("#![forbid(unsafe_code)]\n"));
        assert!(starts_with_the_attribute(
            "//! Docs.\n\n#![forbid(unsafe_code)]\n\nfn main() {}\n"
        ));
    }

    #[test]
    fn a_missing_attribute_fails() {
        assert!(!starts_with_the_attribute(""));
        assert!(!starts_with_the_attribute("fn main() {}\n"));
        assert!(!starts_with_the_attribute("#![deny(unsafe_code)]\n"));
    }

    #[test]
    fn an_attribute_that_does_not_come_first_fails() {
        assert!(!starts_with_the_attribute(
            "mod inner;\n#![forbid(unsafe_code)]\n"
        ));
    }

    #[test]
    fn an_attribute_inside_a_comment_or_a_condition_fails() {
        assert!(!starts_with_the_attribute(
            "// #![forbid(unsafe_code)]\nfn main() {}\n"
        ));
        assert!(!starts_with_the_attribute(
            "/*\n#![forbid(unsafe_code)]\n*/\n"
        ));
        assert!(!starts_with_the_attribute(
            "#![cfg_attr(test, forbid(unsafe_code))]\n"
        ));
    }

    #[test]
    fn every_product_crate_but_the_platform_one_forbids_unsafe() {
        let workspace = Workspace::load().unwrap();
        assert_eq!(check(&workspace).unwrap(), Vec::<String>::new());
    }
}
