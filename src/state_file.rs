use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Replaces `path` with `bytes` atomically: readers see the old file or the new
/// one, never a partial write, and the new contents reach the disk before the
/// rename does.
///
/// This uses `fsync`, not Rust's `sync_all`, which on macOS is `F_FULLFSYNC`:
/// that waits for the drive to flush its whole write cache, and under heavy
/// disk load from other processes one call took seconds and stalled every
/// workspace switch behind it.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_as(path, bytes, Mode::KeepExisting)
}

/// Like [`write_atomic`], but the new file is readable and writable by its owner only (`0600`)
/// from the moment it is created.
pub fn write_atomic_owner_only(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_as(path, bytes, Mode::OwnerOnly)
}

#[derive(Clone, Copy)]
enum Mode {
    /// Keep the replaced file's permissions; a new file gets the process default.
    KeepExisting,
    OwnerOnly,
}

fn write_atomic_as(path: &Path, bytes: &[u8], mode: Mode) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = temporary_path(path);
    let result = write_and_rename(path, &temporary, bytes, mode);
    if result.is_err() {
        drop(fs::remove_file(&temporary));
    }
    result?;
    // The rename is durable once the directory entry is.
    fsync(&File::open(parent)?)
}

fn write_and_rename(path: &Path, temporary: &Path, bytes: &[u8], mode: Mode) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    if matches!(mode, Mode::OwnerOnly) {
        options.mode(0o600);
    }
    let mut file = options.open(temporary)?;
    if matches!(mode, Mode::KeepExisting)
        && let Ok(metadata) = fs::metadata(path)
    {
        file.set_permissions(metadata.permissions())?;
    }
    file.write_all(bytes)?;
    fsync(&file)?;
    drop(file);
    fs::rename(temporary, path)
}

fn temporary_path(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(".{name}.{}.{id}.tmp", std::process::id()))
}

fn fsync(file: &File) -> io::Result<()> {
    // SAFETY: `file` owns a valid open descriptor for the duration of the call.
    if unsafe { libc::fsync(file.as_raw_fd()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_contents_and_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let directory =
            std::env::temp_dir().join(format!("fractile-state-file-{}", std::process::id()));
        let path = directory.join("nested").join("state.json");
        write_atomic(&path, b"one").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let leftovers = fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn owner_only_writes_are_0600_even_over_a_wider_file() {
        use std::os::unix::fs::PermissionsExt;
        let directory =
            std::env::temp_dir().join(format!("fractile-owner-only-{}", std::process::id()));
        let path = directory.join("step");
        write_atomic(&path, b"one").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        write_atomic_owner_only(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
