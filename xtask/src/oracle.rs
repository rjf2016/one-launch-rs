use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::workspace;

const MANIFEST: &str = "oracle/MANIFEST.json";

#[derive(Deserialize)]
struct Manifest {
    files: BTreeMap<String, String>,
}

pub fn check(files: &[String]) -> Result<Vec<String>> {
    let manifest: Manifest = serde_json::from_str(&workspace::read(MANIFEST)?)
        .with_context(|| format!("{MANIFEST} is not the manifest this check reads"))?;
    let mut present = BTreeMap::new();
    for file in files {
        let Some(path) = file.strip_prefix("oracle/") else {
            continue;
        };
        if file == MANIFEST {
            continue;
        }
        if let Ok(bytes) = std::fs::read(workspace::root().join(file)) {
            present.insert(path.to_owned(), sha256(&bytes));
        }
    }
    Ok(differences(&manifest.files, &present))
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn differences(
    manifest: &BTreeMap<String, String>,
    present: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut problems = Vec::new();
    for (path, hash) in manifest {
        match present.get(path) {
            None => problems.push(format!("oracle/{path}: in the manifest, not in the folder")),
            Some(found) if found != hash => {
                problems.push(format!("oracle/{path}: differs from the manifest"));
            }
            Some(_) => {}
        }
    }
    for path in present.keys() {
        if !manifest.contains_key(path) {
            problems.push(format!("oracle/{path}: in the folder, not in the manifest"));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(path, hash)| ((*path).to_owned(), (*hash).to_owned()))
            .collect()
    }

    #[test]
    fn the_hash_is_sha256_in_lower_hex() {
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_folder_that_matches_passes() {
        let manifest = map(&[("PIN", "aa"), ("l1/x.json", "bb")]);
        assert_eq!(differences(&manifest, &manifest), Vec::<String>::new());
    }

    #[test]
    fn a_changed_a_missing_and_an_added_file_each_fail() {
        let manifest = map(&[("PIN", "aa"), ("l1/x.json", "bb")]);
        let present = map(&[("PIN", "cc"), ("l1/y.json", "dd")]);
        assert_eq!(
            differences(&manifest, &present),
            [
                "oracle/PIN: differs from the manifest",
                "oracle/l1/x.json: in the manifest, not in the folder",
                "oracle/l1/y.json: in the folder, not in the manifest",
            ]
        );
    }
}
