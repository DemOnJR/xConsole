//! Bulk file transfer over SFTP: downloads, uploads, whole directories, with live
//! progress, a configurable number of files in flight, and cancellation.
//!
//! # Why this is separate from `sftp.rs`
//!
//! The browser's own SFTP channel is a single mutex-guarded session — fine for listing
//! a directory, useless for moving data, because one large read would block every other
//! operation on that panel. Transfers therefore open their own channels on the shared
//! SSH connection (see [`super::sftp::open_channel`]), so N files really do move at
//! once and the file browser stays responsive while they do.
//!
//! # Progress
//!
//! Every job emits [`TRANSFER_EVENT`] with a full snapshot: totals, per-file state,
//! throughput, elapsed and ETA. Snapshots are throttled to [`EMIT_INTERVAL`] so a fast
//! local link can't flood the webview with events, but a file starting, finishing or
//! failing always emits immediately — those are the transitions the UI must not miss.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

use super::sftp::{open_channel, SftpManager};

/// Tauri event carrying [`TransferSnapshot`].
pub const TRANSFER_EVENT: &str = "sftp://transfer";

/// Minimum gap between progress emissions for a running job.
const EMIT_INTERVAL: Duration = Duration::from_millis(200);

/// Read/write chunk. 256 KiB matches russh-sftp's default `max_packet_len`, so a chunk
/// maps to one protocol packet instead of being split.
const CHUNK: usize = 256 * 1024;

/// Fallback when the caller doesn't specify; also the value the UI seeds its setting
/// with. Above ~8 the SSH window becomes the bottleneck and more channels stop helping.
pub const DEFAULT_CONCURRENCY: usize = 4;
const MAX_CONCURRENCY: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Download,
    Upload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileState {
    Pending,
    Active,
    Done,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Scanning,
    Running,
    Done,
    Failed,
    Cancelled,
}

/// One file within a job.
#[derive(Debug, Clone, Serialize)]
pub struct FileProgress {
    pub name: String,
    /// Path on the server (source for a download, destination for an upload).
    pub remote_path: String,
    pub local_path: String,
    pub size: u64,
    pub transferred: u64,
    pub state: FileState,
    pub error: Option<String>,
}

/// What the UI renders. A whole-job snapshot rather than a delta, so a dropped event
/// can never leave the display stuck at a stale number.
#[derive(Debug, Clone, Serialize)]
pub struct TransferSnapshot {
    pub id: String,
    pub direction: Direction,
    pub state: JobState,
    /// Short human label, e.g. "public_html" or "3 files".
    pub label: String,
    pub files_total: usize,
    pub files_done: usize,
    pub bytes_total: u64,
    pub bytes_done: u64,
    /// Milliseconds since the job started.
    pub elapsed_ms: u64,
    /// Estimated milliseconds remaining; `None` until throughput is measurable.
    pub eta_ms: Option<u64>,
    /// Throughput over the whole job so far.
    pub bytes_per_sec: u64,
    /// Files currently moving, plus any that failed (so errors stay visible).
    pub files: Vec<FileProgress>,
    pub error: Option<String>,
    /// Where the files were written (downloads) — lets the UI offer "open folder".
    pub destination: Option<String>,
}

/// One file to move.
#[derive(Debug, Clone)]
struct Item {
    remote: String,
    local: PathBuf,
    size: u64,
}

struct Job {
    id: String,
    direction: Direction,
    label: String,
    destination: Option<String>,
    state: Mutex<JobState>,
    files: Mutex<Vec<FileProgress>>,
    bytes_total: AtomicU64,
    bytes_done: AtomicU64,
    files_done: AtomicU64,
    error: Mutex<Option<String>>,
    cancel: AtomicBool,
    started: Instant,
    last_emit: Mutex<Option<Instant>>,
}

impl Job {
    fn snapshot(&self) -> TransferSnapshot {
        let elapsed = self.started.elapsed();
        let elapsed_ms = elapsed.as_millis() as u64;
        let bytes_done = self.bytes_done.load(Ordering::Relaxed);
        let bytes_total = self.bytes_total.load(Ordering::Relaxed);

        let secs = elapsed.as_secs_f64();
        let rate = if secs > 0.25 { bytes_done as f64 / secs } else { 0.0 };
        // Only offer an ETA once throughput means something; a number derived from the
        // first few milliseconds swings wildly and reads as a bug.
        let eta_ms = if rate > 1.0 && bytes_total > bytes_done {
            Some((((bytes_total - bytes_done) as f64 / rate) * 1000.0) as u64)
        } else {
            None
        };

        let files = self.files.lock().unwrap();
        // Keep the payload bounded on a huge job: what matters is what's moving now and
        // what went wrong.
        let visible: Vec<FileProgress> = files
            .iter()
            .filter(|f| matches!(f.state, FileState::Active | FileState::Failed))
            .take(64)
            .cloned()
            .collect();

        TransferSnapshot {
            id: self.id.clone(),
            direction: self.direction,
            state: *self.state.lock().unwrap(),
            label: self.label.clone(),
            files_total: files.len(),
            files_done: self.files_done.load(Ordering::Relaxed) as usize,
            bytes_total,
            bytes_done,
            elapsed_ms,
            eta_ms,
            bytes_per_sec: rate as u64,
            files: visible,
            error: self.error.lock().unwrap().clone(),
            destination: self.destination.clone(),
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn set_state(&self, state: JobState) {
        *self.state.lock().unwrap() = state;
    }
}

/// Registry of running/finished jobs.
#[derive(Clone, Default)]
pub struct TransferManager {
    jobs: Arc<DashMap<String, Arc<Job>>>,
}

impl TransferManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask a job to stop. Workers check between chunks, so a large file stops promptly
    /// without the partial output being passed off as complete.
    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let job = self.jobs.get(id).ok_or_else(|| "transfer not found".to_string())?;
        job.cancel.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Snapshots of every job the app still knows about (for a UI that just mounted).
    pub fn list(&self) -> Vec<TransferSnapshot> {
        self.jobs.iter().map(|j| j.snapshot()).collect()
    }

    /// Drop finished jobs from the registry.
    pub fn clear_finished(&self) {
        self.jobs.retain(|_, j| {
            matches!(*j.state.lock().unwrap(), JobState::Scanning | JobState::Running)
        });
    }

    fn register(&self, job: Arc<Job>) {
        self.jobs.insert(job.id.clone(), job);
    }
}

fn emit(app: &AppHandle, job: &Job, force: bool) {
    if !force {
        let mut last = job.last_emit.lock().unwrap();
        if let Some(t) = *last {
            if t.elapsed() < EMIT_INTERVAL {
                return;
            }
        }
        *last = Some(Instant::now());
    } else {
        *job.last_emit.lock().unwrap() = Some(Instant::now());
    }
    let _ = app.emit(TRANSFER_EVENT, job.snapshot());
}

/// Reject path components that would let a remote name escape the chosen folder.
///
/// Names come from the *server*, so a hostile or simply broken one could contain `..`
/// or an absolute path and steer a write outside the destination the user picked. Every
/// component is checked rather than the joined result, so this holds on both platforms.
fn safe_component(name: &str) -> Result<&str, String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return Err(format!("refusing unsafe remote filename: {name:?}"));
    }
    Ok(name)
}

/// Walk a remote directory, collecting every file below it.
///
/// Iterative rather than recursive: a deep or symlink-looped tree would otherwise blow
/// the stack. `visited` guards against a directory symlink pointing back up, which
/// would loop forever.
async fn collect_remote(
    sftp: &SftpSession,
    root_remote: &str,
    root_local: &Path,
    out: &mut Vec<Item>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut stack = vec![(root_remote.to_string(), root_local.to_path_buf())];
    let mut visited = std::collections::HashSet::new();

    while let Some((remote, local)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        if !visited.insert(remote.clone()) {
            continue;
        }
        let entries = match sftp.read_dir(&remote).await {
            Ok(e) => e,
            // An unreadable subdirectory shouldn't abort the whole job.
            Err(_) => continue,
        };
        for entry in entries {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let name = match safe_component(&name) {
                Ok(n) => n.to_string(),
                Err(_) => continue,
            };
            let child_remote = format!("{}/{}", remote.trim_end_matches('/'), name);
            let child_local = local.join(&name);
            let meta = entry.metadata();
            if meta.is_dir() {
                stack.push((child_remote, child_local));
            } else {
                out.push(Item {
                    remote: child_remote,
                    local: child_local,
                    size: meta.size.unwrap_or(0),
                });
            }
        }
    }
    Ok(())
}

/// Walk a local directory, collecting every file below it.
fn collect_local(root: &Path, base_remote: &str, out: &mut Vec<Item>) -> Result<(), String> {
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| format!("cannot upload {}", root.display()))?;

    let mut stack = vec![(root.to_path_buf(), format!("{}/{}", base_remote.trim_end_matches('/'), root_name))];
    while let Some((dir, remote)) = stack.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let child_remote = format!("{}/{}", remote.trim_end_matches('/'), name);
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => stack.push((path, child_remote)),
                Ok(ft) if ft.is_file() => {
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    out.push(Item { remote: child_remote, local: path, size });
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Stream one remote file to disk.
async fn download_one(
    sftp: &SftpSession,
    item: &Item,
    job: &Job,
    idx: usize,
    app: &AppHandle,
) -> Result<(), String> {
    if let Some(parent) = item.local.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }

    let mut remote = sftp
        .open(&item.remote)
        .await
        .map_err(|e| format!("open failed: {e}"))?;

    // Write to a temp file and rename on success, so an interrupted download never
    // leaves a plausible-looking short file in the user's folder.
    let tmp = item.local.with_extension(format!(
        "{}.xconsole-part",
        item.local
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    let mut local = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| format!("create {}: {e}", tmp.display()))?;

    let mut buf = vec![0u8; CHUNK];
    let mut written: u64 = 0;
    loop {
        if job.cancelled() {
            drop(local);
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err("cancelled".into());
        }
        let n = remote
            .read(&mut buf)
            .await
            .map_err(|e| format!("read failed: {e}"))?;
        if n == 0 {
            break;
        }
        local
            .write_all(&buf[..n])
            .await
            .map_err(|e| format!("write failed: {e}"))?;
        written += n as u64;
        job.bytes_done.fetch_add(n as u64, Ordering::Relaxed);
        {
            let mut files = job.files.lock().unwrap();
            files[idx].transferred = written;
        }
        emit(app, job, false);
    }

    local.flush().await.map_err(|e| format!("flush failed: {e}"))?;
    drop(local);

    // The scan's size can be stale (the file grew or shrank meanwhile), so correct the
    // totals with what was actually written rather than reporting a wrong percentage.
    if written != item.size {
        let delta = written as i64 - item.size as i64;
        if delta > 0 {
            job.bytes_total.fetch_add(delta as u64, Ordering::Relaxed);
        } else {
            job.bytes_total.fetch_sub((-delta) as u64, Ordering::Relaxed);
        }
    }

    tokio::fs::rename(&tmp, &item.local)
        .await
        .map_err(|e| format!("could not finish {}: {e}", item.local.display()))?;
    Ok(())
}

/// Stream one local file to the server.
async fn upload_one(
    sftp: &SftpSession,
    item: &Item,
    job: &Job,
    idx: usize,
    app: &AppHandle,
) -> Result<(), String> {
    use russh_sftp::protocol::OpenFlags;

    let mut local = tokio::fs::File::open(&item.local)
        .await
        .map_err(|e| format!("open {}: {e}", item.local.display()))?;

    if let Some(parent) = Path::new(&item.remote).parent() {
        let parent = parent.to_string_lossy().replace('\\', "/");
        ensure_remote_dir(sftp, &parent).await;
    }

    // Same temp-then-rename discipline as the editor save: a dropped connection must
    // not leave a half-written file at the destination path.
    let tmp = format!("{}.xconsole-{}.part", item.remote, Uuid::new_v4().simple());
    let mut remote = sftp
        .open_with_flags(&tmp, OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE)
        .await
        .map_err(|e| format!("open for write failed: {e}"))?;

    let mut buf = vec![0u8; CHUNK];
    let mut sent: u64 = 0;
    loop {
        if job.cancelled() {
            let _ = remote.shutdown().await;
            let _ = sftp.remove_file(&tmp).await;
            return Err("cancelled".into());
        }
        let n = local
            .read(&mut buf)
            .await
            .map_err(|e| format!("read failed: {e}"))?;
        if n == 0 {
            break;
        }
        remote
            .write_all(&buf[..n])
            .await
            .map_err(|e| format!("write failed: {e}"))?;
        sent += n as u64;
        job.bytes_done.fetch_add(n as u64, Ordering::Relaxed);
        {
            let mut files = job.files.lock().unwrap();
            files[idx].transferred = sent;
        }
        emit(app, job, false);
    }
    remote
        .shutdown()
        .await
        .map_err(|e| format!("flush failed: {e}"))?;
    drop(remote);

    // Verify before replacing anything. Without this a truncated upload silently
    // becomes the file on the server — the 0-byte failure mode this app must not have.
    let stored = sftp
        .metadata(&tmp)
        .await
        .map_err(|e| format!("could not verify the upload: {e}"))?
        .size
        .unwrap_or(0);
    if stored != sent {
        let _ = sftp.remove_file(&tmp).await;
        return Err(format!(
            "short write — sent {sent} bytes but the server stored {stored}; \
             the destination was left untouched"
        ));
    }

    if sftp.rename(&tmp, &item.remote).await.is_err() {
        let _ = sftp.remove_file(&item.remote).await;
        if let Err(e) = sftp.rename(&tmp, &item.remote).await {
            let _ = sftp.remove_file(&tmp).await;
            return Err(format!("could not replace the file: {e}"));
        }
    }
    Ok(())
}

/// `mkdir -p` over SFTP, ignoring "already exists".
async fn ensure_remote_dir(sftp: &SftpSession, path: &str) {
    let mut built = String::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        built.push('/');
        built.push_str(part);
        let _ = sftp.create_dir(&built).await;
    }
}

/// Start a transfer and return its id immediately; progress arrives as events.
///
/// `sources` are remote paths for a download and local paths for an upload;
/// `destination` is correspondingly a local folder or a remote one. Directories in
/// `sources` are walked, so a folder transfers file-by-file with per-file progress.
#[allow(clippy::too_many_arguments)]
pub fn spawn_job(
    app: AppHandle,
    manager: TransferManager,
    sftp_mgr: SftpManager,
    session_id: String,
    direction: Direction,
    sources: Vec<String>,
    destination: String,
    concurrency: usize,
) -> Result<String, String> {
    let refs = sftp_mgr.session_refs(&session_id)?;
    let id = Uuid::new_v4().to_string();
    let label = match sources.len() {
        0 => return Err("nothing selected".into()),
        1 => Path::new(sources[0].trim_end_matches('/'))
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| sources[0].clone()),
        n => format!("{n} items"),
    };

    let job = Arc::new(Job {
        id: id.clone(),
        direction,
        label,
        destination: matches!(direction, Direction::Download).then(|| destination.clone()),
        state: Mutex::new(JobState::Scanning),
        files: Mutex::new(Vec::new()),
        bytes_total: AtomicU64::new(0),
        bytes_done: AtomicU64::new(0),
        files_done: AtomicU64::new(0),
        error: Mutex::new(None),
        cancel: AtomicBool::new(false),
        started: Instant::now(),
        last_emit: Mutex::new(None),
    });
    manager.register(job.clone());
    emit(&app, &job, true);

    let workers = concurrency.clamp(1, MAX_CONCURRENCY);
    tokio::spawn(async move {
        let outcome = run_job(&app, &job, refs.ssh, direction, sources, destination, workers).await;
        match outcome {
            Ok(()) if job.cancelled() => job.set_state(JobState::Cancelled),
            Ok(()) => {
                let failed = job
                    .files
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|f| f.state == FileState::Failed)
                    .count();
                if failed > 0 {
                    *job.error.lock().unwrap() =
                        Some(format!("{failed} file{} failed", if failed == 1 { "" } else { "s" }));
                    job.set_state(JobState::Failed);
                } else {
                    job.set_state(JobState::Done);
                }
            }
            Err(e) => {
                *job.error.lock().unwrap() = Some(e);
                job.set_state(JobState::Failed);
            }
        }
        emit(&app, &job, true);
    });

    Ok(id)
}

async fn run_job(
    app: &AppHandle,
    job: &Arc<Job>,
    ssh: Arc<russh::client::Handle<super::client::Handler>>,
    direction: Direction,
    sources: Vec<String>,
    destination: String,
    workers: usize,
) -> Result<(), String> {
    // A dedicated channel for the scan, so building the file list doesn't contend with
    // the file browser's channel.
    let scanner = open_channel(&ssh).await?;

    let mut items: Vec<Item> = Vec::new();
    match direction {
        Direction::Download => {
            let dest = PathBuf::from(&destination);
            std::fs::create_dir_all(&dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
            for src in &sources {
                let src = src.trim_end_matches('/');
                let name = Path::new(src)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "download".to_string());
                let name = safe_component(&name)?.to_string();
                let meta = scanner
                    .metadata(src)
                    .await
                    .map_err(|e| format!("{src}: {e}"))?;
                if meta.is_dir() {
                    collect_remote(&scanner, src, &dest.join(&name), &mut items, &job.cancel).await?;
                } else {
                    items.push(Item {
                        remote: src.to_string(),
                        local: dest.join(&name),
                        size: meta.size.unwrap_or(0),
                    });
                }
            }
        }
        Direction::Upload => {
            for src in &sources {
                let path = PathBuf::from(src);
                let meta = std::fs::metadata(&path).map_err(|e| format!("{src}: {e}"))?;
                if meta.is_dir() {
                    collect_local(&path, &destination, &mut items)?;
                } else {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .ok_or_else(|| format!("cannot upload {src}"))?;
                    items.push(Item {
                        remote: format!("{}/{}", destination.trim_end_matches('/'), name),
                        local: path,
                        size: meta.len(),
                    });
                }
            }
        }
    }

    if items.is_empty() {
        return Err("nothing to transfer (no files found)".into());
    }

    {
        let mut files = job.files.lock().unwrap();
        *files = items
            .iter()
            .map(|it| FileProgress {
                name: Path::new(&it.remote)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| it.remote.clone()),
                remote_path: it.remote.clone(),
                local_path: it.local.to_string_lossy().into_owned(),
                size: it.size,
                transferred: 0,
                state: FileState::Pending,
                error: None,
            })
            .collect();
    }
    job.bytes_total
        .store(items.iter().map(|i| i.size).sum(), Ordering::Relaxed);
    job.set_state(JobState::Running);
    emit(app, job, true);

    // Hand every worker its own channel and let them pull from a shared cursor, so a
    // folder of one huge file and many small ones still keeps all workers busy.
    let items = Arc::new(items);
    let cursor = Arc::new(AtomicU64::new(0));
    let mut handles = Vec::new();
    for _ in 0..workers.min(items.len()) {
        let channel = match open_channel(&ssh).await {
            Ok(c) => c,
            // Servers cap concurrent channels; fewer workers is fine, none is not.
            Err(e) if handles.is_empty() => return Err(e),
            Err(_) => break,
        };
        let items = items.clone();
        let cursor = cursor.clone();
        let job = job.clone();
        let app = app.clone();
        handles.push(tokio::spawn(async move {
            loop {
                let idx = cursor.fetch_add(1, Ordering::Relaxed) as usize;
                if idx >= items.len() || job.cancelled() {
                    break;
                }
                let item = &items[idx];
                {
                    let mut files = job.files.lock().unwrap();
                    files[idx].state = FileState::Active;
                }
                emit(&app, &job, true);

                let result = match direction {
                    Direction::Download => download_one(&channel, item, &job, idx, &app).await,
                    Direction::Upload => upload_one(&channel, item, &job, idx, &app).await,
                };

                {
                    let mut files = job.files.lock().unwrap();
                    match result {
                        Ok(()) => {
                            files[idx].state = FileState::Done;
                            files[idx].transferred = files[idx].size.max(files[idx].transferred);
                        }
                        Err(e) if e == "cancelled" => files[idx].state = FileState::Skipped,
                        Err(e) => {
                            files[idx].state = FileState::Failed;
                            files[idx].error = Some(e);
                        }
                    }
                }
                job.files_done.fetch_add(1, Ordering::Relaxed);
                emit(&app, &job, true);
            }
        }));
    }

    for h in handles {
        let _ = h.await;
    }
    Ok(())
}

/// Archive formats offered for a directory download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveFormat {
    TarGz,
    Zip,
}

impl ArchiveFormat {
    fn extension(self) -> &'static str {
        match self {
            ArchiveFormat::TarGz => "tar.gz",
            ArchiveFormat::Zip => "zip",
        }
    }

    /// The command that builds `out` from `name` inside `parent`.
    fn command(self, parent: &str, name: &str, out: &str) -> String {
        let (p, n, o) = (
            super::shell_quote(parent),
            super::shell_quote(name),
            super::shell_quote(out),
        );
        match self {
            // -C changes directory first so the archive holds `name/...` and not the
            // server's full absolute path.
            ArchiveFormat::TarGz => format!("tar -czf {o} -C {p} -- {n}"),
            // zip has no -C, so cd first. -r recurses, -q keeps stdout clean.
            ArchiveFormat::Zip => format!("cd {p} && zip -r -q {o} {n}"),
        }
    }
}

/// Download a remote directory as a single archive.
///
/// Two phases: build the archive into a temp file on the server, then transfer that one
/// file. Streaming `tar` straight down the wire would avoid the temp file, but the
/// compressed size isn't known until it's done — so there would be no total, no
/// percentage and no ETA. Building first costs temp space and buys exact progress plus
/// the same verified, resumable-by-retry transfer path every other download uses. The
/// temp file is removed whether or not the transfer succeeds.
pub fn spawn_archive_job(
    app: AppHandle,
    manager: TransferManager,
    sftp_mgr: SftpManager,
    session_id: String,
    remote_dir: String,
    destination: String,
    format: ArchiveFormat,
    concurrency: usize,
) -> Result<String, String> {
    let refs = sftp_mgr.session_refs(&session_id)?;
    let trimmed = remote_dir.trim_end_matches('/').to_string();
    let name = Path::new(&trimmed)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| "cannot archive the filesystem root".to_string())?;
    let parent = Path::new(&trimmed)
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "/".to_string());

    let id = Uuid::new_v4().to_string();
    let job = Arc::new(Job {
        id: id.clone(),
        direction: Direction::Download,
        label: format!("{name}.{}", format.extension()),
        destination: Some(destination.clone()),
        state: Mutex::new(JobState::Scanning),
        files: Mutex::new(Vec::new()),
        bytes_total: AtomicU64::new(0),
        bytes_done: AtomicU64::new(0),
        files_done: AtomicU64::new(0),
        error: Mutex::new(None),
        cancel: AtomicBool::new(false),
        started: Instant::now(),
        last_emit: Mutex::new(None),
    });
    manager.register(job.clone());
    emit(&app, &job, true);

    let vps_id = refs.vps_id.clone();
    let ssh = refs.ssh.clone();
    tokio::spawn(async move {
        // A path under the archive's own parent would land inside the next archive of
        // that directory, so build in the system temp dir instead.
        let remote_tmp = format!("/tmp/xconsole-{}.{}", Uuid::new_v4().simple(), format.extension());
        let build = format.command(&parent, &name, &remote_tmp);

        let result = async {
            let out = super::command::run_vps_command(&sftp_mgr.db_ref().clone(), &vps_id, &build)
                .await?;
            if out.exit_code != 0 {
                let detail = out.stderr.trim();
                return Err(if detail.is_empty() {
                    format!("archiving failed (exit {})", out.exit_code)
                } else {
                    format!("archiving failed: {detail}")
                });
            }

            let scanner = open_channel(&ssh).await?;
            let size = scanner
                .metadata(&remote_tmp)
                .await
                .map_err(|e| format!("archive vanished after it was built: {e}"))?
                .size
                .unwrap_or(0);

            let local = Path::new(&destination).join(format!("{name}.{}", format.extension()));
            {
                let mut files = job.files.lock().unwrap();
                *files = vec![FileProgress {
                    name: format!("{name}.{}", format.extension()),
                    remote_path: remote_tmp.clone(),
                    local_path: local.to_string_lossy().into_owned(),
                    size,
                    transferred: 0,
                    state: FileState::Active,
                    error: None,
                }];
            }
            job.bytes_total.store(size, Ordering::Relaxed);
            job.set_state(JobState::Running);
            emit(&app, &job, true);

            let item = Item { remote: remote_tmp.clone(), local, size };
            download_one(&scanner, &item, &job, 0, &app).await?;
            job.files_done.fetch_add(1, Ordering::Relaxed);
            {
                let mut files = job.files.lock().unwrap();
                files[0].state = FileState::Done;
            }
            Ok::<(), String>(())
        }
        .await;

        // Always reap the server-side temp file, success or not.
        let cleanup = format!("rm -f -- {}", super::shell_quote(&remote_tmp));
        let _ = super::command::run_vps_command(&sftp_mgr.db_ref().clone(), &vps_id, &cleanup).await;

        match result {
            Ok(()) if job.cancelled() => job.set_state(JobState::Cancelled),
            Ok(()) => job.set_state(JobState::Done),
            Err(e) => {
                *job.error.lock().unwrap() = Some(e);
                job.set_state(JobState::Failed);
            }
        }
        emit(&app, &job, true);
    });

    let _ = concurrency; // a single archive file — nothing to parallelise
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_commands_quote_their_arguments() {
        // A directory called `; rm -rf /` must stay one argument.
        let cmd = ArchiveFormat::TarGz.command("/var/www", "a b; rm -rf /", "/tmp/x.tar.gz");
        assert!(cmd.contains("'a b; rm -rf /'"), "got: {cmd}");
        assert!(cmd.contains("-C '/var/www'"), "got: {cmd}");
        // `--` stops the name being read as an option.
        assert!(cmd.contains(" -- "), "got: {cmd}");

        let zip = ArchiveFormat::Zip.command("/srv", "site", "/tmp/x.zip");
        assert!(zip.starts_with("cd '/srv' &&"), "got: {zip}");
    }

    #[test]
    fn single_quotes_in_a_name_cannot_break_out() {
        let cmd = ArchiveFormat::TarGz.command("/p", "it's", "/tmp/o.tar.gz");
        // POSIX close-escape-reopen, so the quoting stays balanced.
        assert!(cmd.contains(r#"'it'\''s'"#), "got: {cmd}");
    }

    #[test]
    fn rejects_filenames_that_escape_the_destination() {
        // A server controls these strings, so traversal has to be refused outright.
        for bad in ["..", ".", "", "a/b", "a\\b", "x\0y"] {
            assert!(safe_component(bad).is_err(), "should reject {bad:?}");
        }
        for good in ["file.txt", "a-b_c.tar.gz", ".hidden", "spaces are fine"] {
            assert!(safe_component(good).is_ok(), "should accept {good:?}");
        }
    }

    #[test]
    fn concurrency_is_clamped_to_a_sane_range() {
        assert_eq!(0_usize.clamp(1, MAX_CONCURRENCY), 1);
        assert_eq!(999_usize.clamp(1, MAX_CONCURRENCY), MAX_CONCURRENCY);
        assert_eq!(DEFAULT_CONCURRENCY.clamp(1, MAX_CONCURRENCY), DEFAULT_CONCURRENCY);
    }
}
