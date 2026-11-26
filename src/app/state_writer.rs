use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
    thread,
    time::{Duration, Instant},
};

/// Writes advisory state files (workspace layout, window memory) on its own
/// thread so a busy disk never holds up a workspace switch. Each path keeps
/// only its newest contents: while one write is stuck behind other processes'
/// I/O, later snapshots replace each other and the next write sends the latest.
///
/// Parked-window frames don't go through here: they must be on disk before a
/// window moves off-screen, so they stay synchronous.
pub(super) struct StateWriter {
    shared: Arc<Shared>,
    /// False when the thread couldn't start; writes then happen inline.
    threaded: bool,
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    changed: Condvar,
}

const RETRY_BASE: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(60);

struct PendingWrite {
    bytes: Vec<u8>,
    label: &'static str,
    owner_only: bool,
}

struct FailedWrite {
    bytes: Vec<u8>,
    label: &'static str,
    owner_only: bool,
    attempt: u64,
}

#[derive(Default)]
struct Queue {
    pending: BTreeMap<PathBuf, PendingWrite>,
    failed: BTreeMap<PathBuf, FailedWrite>,
    attempts: u64,
    writing: bool,
    closed: bool,
}

impl Queue {
    fn requeue_failed_through(&mut self, retry_before: u64) -> bool {
        let paths: Vec<PathBuf> = self
            .failed
            .iter()
            .filter(|(_, entry)| entry.attempt <= retry_before)
            .map(|(path, _)| path.clone())
            .collect();
        let mut requeued = false;
        for path in paths {
            if let Some(entry) = self.failed.remove(&path) {
                self.pending.entry(path).or_insert(PendingWrite {
                    bytes: entry.bytes,
                    label: entry.label,
                    owner_only: entry.owner_only,
                });
                requeued = true;
            }
        }
        requeued
    }
}

impl StateWriter {
    pub(super) fn spawn() -> Self {
        let shared = Arc::new(Shared::default());
        let thread_shared = Arc::clone(&shared);
        let threaded = match thread::Builder::new()
            .name("fractile-state-writer".into())
            .spawn(move || run(&thread_shared))
        {
            Ok(_) => true,
            Err(error) => {
                log_event!("state writer unavailable, writing inline: {error}");
                false
            }
        };
        Self { shared, threaded }
    }

    pub(super) fn write(&self, path: PathBuf, bytes: Vec<u8>, label: &'static str) {
        self.enqueue(path, bytes, label, false);
    }

    /// Like [`Self::write`], but writes the file atomically with mode `0600`.
    pub(super) fn write_owner_only(&self, path: PathBuf, bytes: Vec<u8>, label: &'static str) {
        self.enqueue(path, bytes, label, true);
    }

    fn enqueue(&self, path: PathBuf, bytes: Vec<u8>, label: &'static str, owner_only: bool) {
        if !self.threaded {
            let mut queue = self.shared.lock();
            queue.attempts += 1;
            let attempt = queue.attempts;
            if write(&path, &bytes, label, owner_only) {
                queue.failed.remove(&path);
            } else {
                queue.failed.insert(
                    path,
                    FailedWrite {
                        bytes,
                        label,
                        owner_only,
                        attempt,
                    },
                );
            }
            return;
        }
        let mut queue = self.shared.lock();
        queue.failed.remove(&path);
        queue.pending.insert(
            path,
            PendingWrite {
                bytes,
                label,
                owner_only,
            },
        );
        drop(queue);
        self.shared.changed.notify_all();
    }

    /// Retries any previously failed snapshots and waits up to `timeout` for
    /// every queued write to land. Returns `false` if any writes remain pending
    /// or still failed.
    pub(super) fn flush(&self, timeout: Duration) -> bool {
        if !self.threaded {
            let mut queue = self.shared.lock();
            let failed = std::mem::take(&mut queue.failed);
            for (path, entry) in failed {
                queue.attempts += 1;
                let attempt = queue.attempts;
                if !write(&path, &entry.bytes, entry.label, entry.owner_only) {
                    queue.failed.insert(path, FailedWrite { attempt, ..entry });
                }
            }
            return queue.failed.is_empty();
        }
        let deadline = Instant::now() + timeout;
        let mut queue = self.shared.lock();
        let retry_before = queue.attempts;
        loop {
            if queue.requeue_failed_through(retry_before) {
                self.shared.changed.notify_all();
            }
            if queue.pending.is_empty() && !queue.writing {
                return queue.failed.is_empty();
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            queue = self
                .shared
                .changed
                .wait_timeout(queue, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl Drop for StateWriter {
    fn drop(&mut self) {
        self.shared.lock().closed = true;
        self.shared.changed.notify_all();
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn run(shared: &Shared) {
    let mut queue = shared.lock();
    let mut retry_delay = RETRY_BASE;
    let mut next_retry: Option<Instant> = None;
    loop {
        let Some((
            path,
            PendingWrite {
                bytes,
                label,
                owner_only,
            },
        )) = queue.pending.pop_first()
        else {
            if queue.closed {
                return;
            }
            if queue.failed.is_empty() {
                next_retry = None;
                retry_delay = RETRY_BASE;
                queue = shared
                    .changed
                    .wait(queue)
                    .unwrap_or_else(PoisonError::into_inner);
            } else {
                let retry_at = *next_retry.get_or_insert_with(|| Instant::now() + retry_delay);
                if let Some(remaining) = retry_at.checked_duration_since(Instant::now()) {
                    queue = shared
                        .changed
                        .wait_timeout(queue, remaining)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                } else {
                    let latest = queue.attempts;
                    queue.requeue_failed_through(latest);
                    next_retry = None;
                    retry_delay = (retry_delay * 2).min(RETRY_MAX);
                }
            }
            continue;
        };
        queue.attempts += 1;
        let attempt = queue.attempts;
        queue.writing = true;
        drop(queue);
        let ok = write(&path, &bytes, label, owner_only);
        queue = shared.lock();
        queue.writing = false;
        if ok {
            retry_delay = RETRY_BASE;
            next_retry = None;
        } else if !queue.pending.contains_key(&path) {
            queue.failed.insert(
                path,
                FailedWrite {
                    bytes,
                    label,
                    owner_only,
                    attempt,
                },
            );
        }
        shared.changed.notify_all();
    }
}

fn write(path: &Path, bytes: &[u8], label: &str, owner_only: bool) -> bool {
    let started = Instant::now();
    let result = if owner_only {
        crate::state_file::write_atomic_owner_only(path, bytes)
    } else {
        crate::state_file::write_atomic(path, bytes)
    };
    let ok = match result {
        Ok(()) => true,
        Err(error) => {
            log_event!("failed to write {label} state: {error}");
            false
        }
    };
    let elapsed = started.elapsed();
    if elapsed >= Duration::from_millis(500) {
        log_event!("slow {label} state write: {}ms", elapsed.as_millis());
    }
    ok
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "fractile-state-writer-{name}-{}",
            std::process::id()
        ));
        drop(std::fs::remove_dir_all(&directory));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn writes_the_latest_contents_and_flushes() {
        let directory = temp_dir("latest");
        let path = directory.join("state.json");
        let writer = StateWriter::spawn();
        for value in 0..50 {
            writer.write(path.clone(), value.to_string().into_bytes(), "test");
        }
        assert!(writer.flush(Duration::from_secs(10)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "49");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_write_is_retried_once_storage_recovers() {
        let directory = temp_dir("retry");
        let path = directory.join("workspace-state.json");
        let writer = StateWriter::spawn();
        let timeout = Duration::from_secs(2);

        writer.write(path.clone(), b"previous".to_vec(), "workspace");
        assert!(writer.flush(timeout));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous");

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o500)).unwrap();
        writer.write(path.clone(), b"latest".to_vec(), "workspace");
        assert!(!writer.flush(timeout));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous");

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(writer.flush(timeout));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "latest");

        writer.write(path.clone(), b"control".to_vec(), "workspace");
        assert!(writer.flush(timeout));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "control");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn newer_write_supersedes_a_failed_snapshot() {
        let directory = temp_dir("supersede");
        let path = directory.join("workspace-state.json");
        let writer = StateWriter::spawn();
        let timeout = Duration::from_secs(2);

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o500)).unwrap();
        writer.write(path.clone(), b"stale".to_vec(), "workspace");
        assert!(!writer.flush(timeout));

        writer.write(path.clone(), b"newer".to_vec(), "workspace");
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(writer.flush(timeout));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "newer");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn inline_writer_retries_failed_writes_on_flush() {
        let directory = temp_dir("inline");
        let path = directory.join("workspace-state.json");
        let writer = StateWriter {
            shared: Arc::new(Shared::default()),
            threaded: false,
        };
        let timeout = Duration::from_secs(2);

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o500)).unwrap();
        writer.write(path.clone(), b"latest".to_vec(), "workspace");
        assert!(!writer.flush(timeout));

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(writer.flush(timeout));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "latest");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn owner_only_writes_set_mode_0600_including_after_retry() {
        let directory = temp_dir("owner-only");
        let path = directory.join("window-memory.json");
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let writer = StateWriter::spawn();
        let timeout = Duration::from_secs(2);
        writer.write_owner_only(path.clone(), b"first".to_vec(), "window memory");
        assert!(writer.flush(timeout));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o500)).unwrap();
        writer.write_owner_only(path.clone(), b"retried".to_vec(), "window memory");
        assert!(!writer.flush(timeout));

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(writer.flush(timeout));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "retried");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
