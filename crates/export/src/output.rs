use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::{ExportError, Result};

/// A file being exported. Bytes go to `<target>.part`; [`Output::commit`] renames it onto the
/// target. Dropping an `Output` that was not committed deletes the `.part` file, so a cancelled
/// or failed export leaves nothing behind, and an existing target stays as it was.
#[derive(Debug)]
pub struct Output {
    target: PathBuf,
    part: PathBuf,
    file: Option<BufWriter<File>>,
    is_committed: bool,
}

impl Output {
    /// Open `<target>.part`. A `.part` file left by a crashed export is overwritten.
    pub fn create(target: &Path) -> Result<Self> {
        let mut part = target.as_os_str().to_owned();
        part.push(".part");
        let part = PathBuf::from(part);
        let file = File::create(&part).map_err(|err| match err.kind() {
            io::ErrorKind::NotFound => ExportError::FolderNotFound(folder_of(target)),
            io::ErrorKind::PermissionDenied => ExportError::PermissionDenied(folder_of(target)),
            _ => ExportError::from(err),
        })?;
        Ok(Self {
            target: target.to_owned(),
            part,
            file: Some(BufWriter::new(file)),
            is_committed: false,
        })
    }

    /// Flush to disk and rename onto the target.
    pub fn commit(mut self) -> Result<()> {
        let writer = self
            .file
            .take()
            .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?;
        let file = writer.into_inner().map_err(|err| err.into_error())?;
        file.sync_all()?;
        // Windows cannot rename a file that is still open.
        drop(file);
        fs::rename(&self.part, &self.target)?;
        self.is_committed = true;
        Ok(())
    }

    /// Give up: delete the `.part` file. Dropping does the same.
    pub fn abort(self) {}
}

impl Write for Output {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.file.as_mut() {
            Some(file) => file.write(buf),
            None => Err(io::ErrorKind::BrokenPipe.into()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.flush(),
            None => Err(io::ErrorKind::BrokenPipe.into()),
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        if self.is_committed {
            return;
        }
        self.file = None;
        // Nothing to report: the export has already ended, and a leftover `.part` is
        // overwritten by the next export to the same path.
        let _ = fs::remove_file(&self.part);
    }
}

fn folder_of(target: &Path) -> PathBuf {
    match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_owned(),
        _ => PathBuf::from("."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::temp_dir;

    #[test]
    fn commit_renames_part_onto_the_target() {
        let dir = temp_dir();
        let target = dir.join("out.csv");
        let mut output = Output::create(&target).unwrap();
        output.write_all(b"a,b\r\n").unwrap();
        assert!(!target.exists());
        assert!(dir.join("out.csv.part").exists());
        output.commit().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"a,b\r\n");
        assert!(!dir.join("out.csv.part").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn abort_removes_part_and_keeps_an_existing_target() {
        let dir = temp_dir();
        let target = dir.join("out.csv");
        fs::write(&target, b"old").unwrap();
        let mut output = Output::create(&target).unwrap();
        output.write_all(b"new").unwrap();
        output.abort();
        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert!(!dir.join("out.csv.part").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dropping_without_commit_leaves_no_file() {
        let dir = temp_dir();
        let target = dir.join("out.csv");
        {
            let mut output = Output::create(&target).unwrap();
            output.write_all(b"partial").unwrap();
        }
        assert!(!target.exists());
        assert!(!dir.join("out.csv.part").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_folder_is_named_and_creates_no_file() {
        let dir = temp_dir();
        let missing = dir.join("nope");
        let err = Output::create(&missing.join("out.csv")).unwrap_err();
        match &err {
            ExportError::FolderNotFound(folder) => assert_eq!(folder, &missing),
            other => panic!("expected FolderNotFound, got {other:?}"),
        }
        assert!(err.to_string().contains("Choose a folder"));
        assert!(!missing.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_stale_part_file_is_overwritten() {
        let dir = temp_dir();
        let target = dir.join("out.csv");
        fs::write(dir.join("out.csv.part"), b"left by a crash, much longer").unwrap();
        let mut output = Output::create(&target).unwrap();
        output.write_all(b"ok").unwrap();
        output.commit().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"ok");
        fs::remove_dir_all(dir).unwrap();
    }
}
