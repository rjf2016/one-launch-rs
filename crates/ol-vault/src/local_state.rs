use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value};

use crate::KeyError;

const WRAPPED_PREFIX: &[u8; 5] = b"DPAPI";
const LARGEST: u64 = 16 * 1024 * 1024;

pub fn local_state_path(data_dir: &Path) -> PathBuf {
    data_dir.join("chromium").join("Local State")
}

pub(crate) fn read_key(file: &Path) -> Result<Option<Vec<u8>>, KeyError> {
    let Some(state) = read(file)? else {
        return Ok(None);
    };
    let Some(os_crypt) = state.get("os_crypt") else {
        return Ok(None);
    };
    let entry = match os_crypt {
        Value::Object(os_crypt) => os_crypt.get("encrypted_key"),
        _ => return Err(KeyError::Malformed),
    };
    let text = match entry {
        None => return Ok(None),
        Some(Value::String(text)) => text,
        Some(_) => return Err(KeyError::Malformed),
    };
    let decoded = STANDARD.decode(text).map_err(|_| KeyError::Malformed)?;
    match decoded.strip_prefix(WRAPPED_PREFIX) {
        Some(wrapped) if !wrapped.is_empty() => Ok(Some(wrapped.to_vec())),
        _ => Err(KeyError::Malformed),
    }
}

// Whatever else the file holds is Chromium's and is written back as it was. A key that is already there is never replaced: everything stored so far opens with it alone.
pub(crate) fn write_key(file: &Path, wrapped: &[u8]) -> Result<(), KeyError> {
    let mut state = read(file)?.unwrap_or_default();
    let os_crypt = state
        .entry("os_crypt")
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(os_crypt) = os_crypt else {
        return Err(KeyError::Malformed);
    };
    if os_crypt.contains_key("encrypted_key") {
        return Err(KeyError::Write(io::ErrorKind::AlreadyExists.into()));
    }
    let key = [WRAPPED_PREFIX.as_slice(), wrapped].concat();
    os_crypt.insert(
        "encrypted_key".to_owned(),
        Value::String(STANDARD.encode(key)),
    );
    let text = serde_json::to_vec(&state).map_err(|_| KeyError::Malformed)?;

    let folder = file.parent().ok_or(KeyError::Malformed)?;
    fs::create_dir_all(folder).map_err(KeyError::Write)?;
    let unfinished = folder.join(format!("Local State.onelaunch-{}.tmp", std::process::id()));
    let written = (|| {
        let mut target = fs::File::create(&unfinished)?;
        target.write_all(&text)?;
        target.sync_all()?;
        drop(target);
        fs::rename(&unfinished, file)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&unfinished);
    }
    written.map_err(KeyError::Write)
}

fn read(file: &Path) -> Result<Option<Map<String, Value>>, KeyError> {
    let source = match fs::File::open(file) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(KeyError::Read(error)),
    };
    let mut text = Vec::new();
    source
        .take(LARGEST + 1)
        .read_to_end(&mut text)
        .map_err(KeyError::Read)?;
    if text.len() as u64 > LARGEST {
        return Err(KeyError::Malformed);
    }
    match serde_json::from_slice(&text) {
        Ok(Value::Object(state)) => Ok(Some(state)),
        _ => Err(KeyError::Malformed),
    }
}

#[cfg(test)]
mod tests {
    use ol_testkit::{Scratch, snapshot};

    use super::*;

    #[test]
    fn a_key_that_is_there_is_not_written_over() {
        let scratch = Scratch::new("vault");
        let file = local_state_path(scratch.path());
        write_key(&file, &[1; 40]).unwrap();
        let before = snapshot(scratch.path());
        assert_eq!(read_key(&file).unwrap(), Some(vec![1; 40]));

        let refused = write_key(&file, &[2; 40]);
        assert!(
            matches!(&refused, Err(KeyError::Write(error)) if error.kind() == io::ErrorKind::AlreadyExists),
            "{refused:?}"
        );
        assert!(before == snapshot(scratch.path()), "the folder changed");
    }
}
