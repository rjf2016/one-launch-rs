use anyhow::{Context, Result, bail};

use crate::workspace;

const PINS: &str = "tools.toml";

pub fn require(tool: &str) -> Result<()> {
    let pinned = pinned(&workspace::read(PINS)?, tool)
        .with_context(|| format!("{PINS} does not pin {tool}"))?;
    let subcommand = tool.strip_prefix("cargo-").unwrap_or(tool);
    let printed = workspace::output("cargo", &[subcommand, "--version"])
        .with_context(|| format!("{tool} {pinned} is not installed"))?;
    let printed = String::from_utf8_lossy(&printed);
    let installed = printed.split_whitespace().nth(1).unwrap_or_default();
    if installed != pinned {
        bail!("{tool} {installed} is installed, {PINS} pins {pinned}");
    }
    Ok(())
}

fn pinned(pins: &str, tool: &str) -> Option<String> {
    pins.lines().find_map(|line| {
        let (name, version) = line.split_once('=')?;
        (name.trim() == tool).then(|| version.trim().trim_matches('"').to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PINS: &str = "[tools]\ncargo-deny = \"0.20.2\"\ncargo-nextest = \"0.9.152\"\n";

    #[test]
    fn a_pinned_version_is_read() {
        assert_eq!(pinned(PINS, "cargo-deny").as_deref(), Some("0.20.2"));
        assert_eq!(pinned(PINS, "cargo-nextest").as_deref(), Some("0.9.152"));
    }

    #[test]
    fn a_tool_that_is_not_pinned_is_none() {
        assert_eq!(pinned(PINS, "cargo-audit"), None);
        assert_eq!(pinned(PINS, "cargo"), None);
    }
}
