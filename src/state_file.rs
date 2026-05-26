use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};

static WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn write_atomically(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    write_atomically_with_mode(path, contents.as_ref(), None)
}

#[cfg(unix)]
pub(crate) fn write_private_atomically(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    write_atomically_with_mode(path, contents.as_ref(), Some(0o600))
}

#[cfg(not(unix))]
pub(crate) fn write_private_atomically(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    write_atomically(path, contents)
}

fn write_atomically_with_mode(path: &Path, contents: &[u8], mode: Option<u32>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let temp_path = unique_temp_path(path);
    let mut temp_file = TempFile::new(temp_path);
    let mut file = fs::File::create(temp_file.path()).with_context(|| {
        format!(
            "failed to write temporary state file {}",
            temp_file.path().display()
        )
    })?;

    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(temp_file.path(), fs::Permissions::from_mode(mode)).with_context(
            || {
                format!(
                    "failed to set private permissions on {}",
                    temp_file.path().display()
                )
            },
        )?;
    }

    #[cfg(not(unix))]
    let _ = mode;

    file.write_all(contents)
        .with_context(|| format!("failed to write {}", temp_file.path().display()))?;
    file.flush()
        .with_context(|| format!("failed to flush {}", temp_file.path().display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync {}", temp_file.path().display()))?;
    drop(file);

    replace_file(temp_file.path(), path).with_context(|| {
        format!(
            "failed to replace {} with {}",
            path.display(),
            temp_file.path().display()
        )
    })?;
    temp_file.persist();
    Ok(())
}

fn unique_temp_path(path: &Path) -> PathBuf {
    let sequence = WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("state-file");
    path.with_file_name(format!(
        ".{file_name}.{}.{}.part",
        std::process::id(),
        sequence
    ))
}

fn replace_file(from: &Path, to: &Path) -> Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            fs::remove_file(to)
                .with_context(|| format!("failed to remove old file {}", to.display()))?;
            fs::rename(from, to)
                .with_context(|| format!("failed to move {} to {}", from.display(), to.display()))
        }
        Err(error) => Err(error)
            .with_context(|| format!("failed to move {} to {}", from.display(), to.display())),
    }
}

struct TempFile {
    path: PathBuf,
    persisted: bool,
}

impl TempFile {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            persisted: false,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn persist(&mut self) {
        self.persisted = true;
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.persisted {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_existing_file_without_part_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("state.txt");
        fs::write(&path, "old").unwrap();

        write_atomically(&path, "new").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        let entries = fs::read_dir(tmp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec!["state.txt"]);
    }

    #[cfg(unix)]
    #[test]
    fn private_atomic_write_uses_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secret.txt");

        write_private_atomically(&path, "secret").unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
