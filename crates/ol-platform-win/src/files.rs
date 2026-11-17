use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use ol_platform_api::{FileReader, FileWriter};
use time::OffsetDateTime;

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsFiles;

impl FileReader for WindowsFiles {
    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn size(&self, path: &str) -> io::Result<u64> {
        Ok(fs::metadata(path)?.len())
    }

    fn modified(&self, path: &str) -> io::Result<OffsetDateTime> {
        Ok(fs::metadata(path)?.modified()?.into())
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn read_range(&self, path: &str, from: u64, length: u64) -> io::Result<Vec<u8>> {
        let mut file = fs::File::open(path)?;
        file.seek(SeekFrom::Start(from))?;
        let mut bytes = Vec::new();
        file.take(length).read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn list(&self, folder: &str) -> io::Result<Vec<String>> {
        fs::read_dir(folder)?
            .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
            .collect()
    }

    // As the Electron app's realpath gives it: a drive path, or a share as \\server\share.
    fn resolved(&self, folder: &str) -> Option<String> {
        let real = fs::canonicalize(folder).ok()?;
        let real = real.to_string_lossy();
        Some(if let Some(share) = real.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{share}")
        } else {
            real.strip_prefix(r"\\?\").unwrap_or(&real).to_owned()
        })
    }
}

impl FileWriter for WindowsFiles {
    fn write_atomic(&self, files: &[(&str, &[u8])]) -> io::Result<()> {
        let staged: Vec<(&Path, &[u8])> = files
            .iter()
            .map(|(path, bytes)| (Path::new(*path), *bytes))
            .collect();
        ol_platform_api::atomic::write_files(&staged)
    }
}
