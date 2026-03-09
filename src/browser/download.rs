use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

/// Unique identifier for a download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DownloadId(u64);

impl DownloadId {
    /// Generate a fresh download id.
    pub fn next() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Return the raw id value.
    #[must_use]
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// State of a download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadState {
    /// Download is queued but not started.
    Pending,
    /// Download is actively transferring data.
    InProgress,
    /// Download paused by the user.
    Paused,
    /// Download completed successfully.
    Completed,
    /// Download failed with an error.
    Failed,
    /// Download cancelled by the user.
    Cancelled,
}

impl DownloadState {
    /// Whether this download is still active (pending, in progress, or paused).
    #[must_use]
    pub fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::InProgress | Self::Paused)
    }

    /// Whether this download is in a terminal state.
    #[must_use]
    pub fn is_finished(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// A single download entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Download {
    /// Unique download identifier.
    pub id: DownloadId,
    /// The source URL.
    #[serde(with = "crate::browser::bookmark::url_serde")]
    pub url: Url,
    /// Filename (extracted from URL or Content-Disposition).
    pub filename: String,
    /// Local file path where the download is saved.
    pub path: PathBuf,
    /// MIME type, if known.
    pub mime_type: Option<String>,
    /// Bytes downloaded so far.
    pub bytes_downloaded: u64,
    /// Total bytes, if known from Content-Length.
    pub total_bytes: Option<u64>,
    /// Current state.
    pub state: DownloadState,
    /// Unix timestamp when the download was started.
    pub started_at: i64,
    /// Unix timestamp when the download completed (or failed/cancelled).
    pub finished_at: Option<i64>,
    /// Error message if the download failed.
    pub error: Option<String>,
}

impl Download {
    /// Create a new pending download.
    pub fn new(url: Url, filename: impl Into<String>, path: PathBuf) -> Self {
        Self {
            id: DownloadId::next(),
            url,
            filename: filename.into(),
            path,
            mime_type: None,
            bytes_downloaded: 0,
            total_bytes: None,
            state: DownloadState::Pending,
            started_at: chrono::Utc::now().timestamp(),
            finished_at: None,
            error: None,
        }
    }

    /// Progress as a fraction (0.0 to 1.0), or `None` if total is unknown.
    #[must_use]
    pub fn progress(&self) -> Option<f32> {
        self.total_bytes.map(|total| {
            if total == 0 {
                1.0
            } else {
                #[allow(clippy::cast_precision_loss)]
                {
                    self.bytes_downloaded as f32 / total as f32
                }
            }
        })
    }

    /// Human-readable progress string (e.g. "1.2 MB / 5.0 MB").
    #[must_use]
    pub fn progress_text(&self) -> String {
        let downloaded = format_bytes(self.bytes_downloaded);
        match self.total_bytes {
            Some(total) => format!("{downloaded} / {}", format_bytes(total)),
            None => downloaded,
        }
    }
}

/// Manages active and completed downloads.
#[derive(Debug, Clone)]
pub struct DownloadManager {
    /// All downloads (active and completed).
    downloads: Vec<Download>,
    /// Default download directory.
    pub download_dir: PathBuf,
    /// Whether to ask the user for a download location.
    pub ask_location: bool,
}

impl DownloadManager {
    /// Create a new download manager with the given default directory.
    pub fn new(download_dir: PathBuf) -> Self {
        Self {
            downloads: Vec::new(),
            download_dir,
            ask_location: false,
        }
    }

    /// Start a new download. Returns the download id.
    pub fn start_download(&mut self, url: Url, filename: impl Into<String>) -> DownloadId {
        let filename = filename.into();
        let path = self.download_dir.join(&filename);
        let mut download = Download::new(url, filename, path);
        download.state = DownloadState::InProgress;
        let id = download.id;
        self.downloads.push(download);
        id
    }

    /// Update download progress.
    pub fn update_progress(
        &mut self,
        id: DownloadId,
        bytes_downloaded: u64,
        total_bytes: Option<u64>,
    ) {
        if let Some(dl) = self.get_mut(id) {
            dl.bytes_downloaded = bytes_downloaded;
            if let Some(total) = total_bytes {
                dl.total_bytes = Some(total);
            }
        }
    }

    /// Mark a download as completed.
    pub fn complete(&mut self, id: DownloadId) {
        if let Some(dl) = self.get_mut(id) {
            dl.state = DownloadState::Completed;
            dl.finished_at = Some(chrono::Utc::now().timestamp());
            if let Some(total) = dl.total_bytes {
                dl.bytes_downloaded = total;
            }
        }
    }

    /// Mark a download as failed.
    pub fn fail(&mut self, id: DownloadId, error: impl Into<String>) {
        if let Some(dl) = self.get_mut(id) {
            dl.state = DownloadState::Failed;
            dl.finished_at = Some(chrono::Utc::now().timestamp());
            dl.error = Some(error.into());
        }
    }

    /// Cancel a download.
    pub fn cancel(&mut self, id: DownloadId) {
        if let Some(dl) = self.get_mut(id) {
            if dl.state.is_active() {
                dl.state = DownloadState::Cancelled;
                dl.finished_at = Some(chrono::Utc::now().timestamp());
            }
        }
    }

    /// Pause a download.
    pub fn pause(&mut self, id: DownloadId) {
        if let Some(dl) = self.get_mut(id) {
            if dl.state == DownloadState::InProgress {
                dl.state = DownloadState::Paused;
            }
        }
    }

    /// Resume a paused download.
    pub fn resume(&mut self, id: DownloadId) {
        if let Some(dl) = self.get_mut(id) {
            if dl.state == DownloadState::Paused {
                dl.state = DownloadState::InProgress;
            }
        }
    }

    /// Get a download by id.
    #[must_use]
    pub fn get(&self, id: DownloadId) -> Option<&Download> {
        self.downloads.iter().find(|d| d.id == id)
    }

    /// Get a mutable reference to a download by id.
    pub fn get_mut(&mut self, id: DownloadId) -> Option<&mut Download> {
        self.downloads.iter_mut().find(|d| d.id == id)
    }

    /// List all active (non-finished) downloads.
    #[must_use]
    pub fn active(&self) -> Vec<&Download> {
        self.downloads
            .iter()
            .filter(|d| d.state.is_active())
            .collect()
    }

    /// List completed downloads.
    #[must_use]
    pub fn completed(&self) -> Vec<&Download> {
        self.downloads
            .iter()
            .filter(|d| d.state == DownloadState::Completed)
            .collect()
    }

    /// List all downloads (most recent first).
    #[must_use]
    pub fn all(&self) -> &[Download] {
        &self.downloads
    }

    /// Number of currently active downloads.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.downloads
            .iter()
            .filter(|d| d.state.is_active())
            .count()
    }

    /// Remove all completed/failed/cancelled downloads from the list.
    pub fn clear_finished(&mut self) {
        self.downloads.retain(|d| d.state.is_active());
    }

    /// Remove a specific download from the list.
    pub fn remove(&mut self, id: DownloadId) {
        self.downloads.retain(|d| d.id != id);
    }
}

impl Default for DownloadManager {
    fn default() -> Self {
        let download_dir = dirs_next()
            .unwrap_or_else(|| PathBuf::from("."));
        Self::new(download_dir)
    }
}

/// Best-effort determination of the user's Downloads directory.
fn dirs_next() -> Option<PathBuf> {
    // Use $HOME/Downloads as default
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join("Downloads"))
}

/// Format bytes into a human-readable string (KB, MB, GB).
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        #[allow(clippy::cast_precision_loss)]
        let gb = bytes as f64 / GB as f64;
        format!("{gb:.1} GB")
    } else if bytes >= MB {
        #[allow(clippy::cast_precision_loss)]
        let mb = bytes as f64 / MB as f64;
        format!("{mb:.1} MB")
    } else if bytes >= KB {
        #[allow(clippy::cast_precision_loss)]
        let kb = bytes as f64 / KB as f64;
        format!("{kb:.1} KB")
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn download_lifecycle() {
        let mut mgr = DownloadManager::new(PathBuf::from("/tmp/downloads"));

        let id = mgr.start_download(
            test_url("https://example.com/file.zip"),
            "file.zip",
        );

        assert_eq!(mgr.active_count(), 1);
        assert_eq!(mgr.get(id).unwrap().state, DownloadState::InProgress);

        mgr.update_progress(id, 500, Some(1000));
        let dl = mgr.get(id).unwrap();
        assert_eq!(dl.bytes_downloaded, 500);
        assert!((dl.progress().unwrap() - 0.5).abs() < f32::EPSILON);

        mgr.complete(id);
        assert_eq!(mgr.get(id).unwrap().state, DownloadState::Completed);
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn pause_resume() {
        let mut mgr = DownloadManager::new(PathBuf::from("/tmp"));
        let id = mgr.start_download(test_url("https://example.com/f"), "f");

        mgr.pause(id);
        assert_eq!(mgr.get(id).unwrap().state, DownloadState::Paused);
        assert!(mgr.get(id).unwrap().state.is_active());

        mgr.resume(id);
        assert_eq!(mgr.get(id).unwrap().state, DownloadState::InProgress);
    }

    #[test]
    fn cancel_download() {
        let mut mgr = DownloadManager::new(PathBuf::from("/tmp"));
        let id = mgr.start_download(test_url("https://example.com/f"), "f");

        mgr.cancel(id);
        assert_eq!(mgr.get(id).unwrap().state, DownloadState::Cancelled);
        assert!(mgr.get(id).unwrap().state.is_finished());
    }

    #[test]
    fn fail_download() {
        let mut mgr = DownloadManager::new(PathBuf::from("/tmp"));
        let id = mgr.start_download(test_url("https://example.com/f"), "f");

        mgr.fail(id, "network timeout");
        let dl = mgr.get(id).unwrap();
        assert_eq!(dl.state, DownloadState::Failed);
        assert_eq!(dl.error.as_deref(), Some("network timeout"));
    }

    #[test]
    fn clear_finished() {
        let mut mgr = DownloadManager::new(PathBuf::from("/tmp"));
        let id1 = mgr.start_download(test_url("https://example.com/a"), "a");
        let _id2 = mgr.start_download(test_url("https://example.com/b"), "b");

        mgr.complete(id1);
        mgr.clear_finished();
        assert_eq!(mgr.all().len(), 1);
    }

    #[test]
    fn progress_text() {
        let dl = Download {
            id: DownloadId::next(),
            url: test_url("https://example.com/file"),
            filename: "file".into(),
            path: PathBuf::from("/tmp/file"),
            mime_type: None,
            bytes_downloaded: 1_500_000,
            total_bytes: Some(5_000_000),
            state: DownloadState::InProgress,
            started_at: 0,
            finished_at: None,
            error: None,
        };

        let text = dl.progress_text();
        assert!(text.contains("MB"));
    }

    #[test]
    fn format_bytes_units() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
        assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
    }
}
