//! Whisper model manager for downloading and managing GGML models
//!
//! Downloads models from Hugging Face with progress tracking and resume support.

#![allow(dead_code)]

use hf_hub::api::tokio::{ApiBuilder, Progress};
use hf_hub::Cache;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Available Whisper model variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WhisperModel {
    Tiny,
    TinyEn,
    Base,
    BaseEn,
    Small,
    SmallEn,
    Medium,
    MediumEn,
    LargeV1,
    LargeV2,
    LargeV3,
    LargeV3Turbo,
}

impl WhisperModel {
    /// Get all available models
    pub fn all() -> &'static [WhisperModel] {
        &[
            WhisperModel::Tiny,
            WhisperModel::TinyEn,
            WhisperModel::Base,
            WhisperModel::BaseEn,
            WhisperModel::Small,
            WhisperModel::SmallEn,
            WhisperModel::Medium,
            WhisperModel::MediumEn,
            WhisperModel::LargeV1,
            WhisperModel::LargeV2,
            WhisperModel::LargeV3,
            WhisperModel::LargeV3Turbo,
        ]
    }

    /// Get recommended models for typical use
    pub fn recommended() -> &'static [WhisperModel] {
        &[
            WhisperModel::Tiny,
            WhisperModel::Base,
            WhisperModel::Small,
            WhisperModel::Medium,
        ]
    }

    /// Get the default model
    pub fn default_model() -> WhisperModel {
        WhisperModel::Tiny
    }

    /// Get display name for the model
    pub fn display_name(&self) -> &'static str {
        match self {
            WhisperModel::Tiny => "Tiny (75 MB)",
            WhisperModel::TinyEn => "Tiny English (75 MB)",
            WhisperModel::Base => "Base (142 MB)",
            WhisperModel::BaseEn => "Base English (142 MB)",
            WhisperModel::Small => "Small (466 MB)",
            WhisperModel::SmallEn => "Small English (466 MB)",
            WhisperModel::Medium => "Medium (1.5 GB)",
            WhisperModel::MediumEn => "Medium English (1.5 GB)",
            WhisperModel::LargeV1 => "Large v1 (2.9 GB)",
            WhisperModel::LargeV2 => "Large v2 (2.9 GB)",
            WhisperModel::LargeV3 => "Large v3 (2.9 GB)",
            WhisperModel::LargeV3Turbo => "Large v3 Turbo (1.6 GB)",
        }
    }

    /// Get the model name used in filenames
    pub fn file_name(&self) -> &'static str {
        match self {
            WhisperModel::Tiny => "ggml-tiny.bin",
            WhisperModel::TinyEn => "ggml-tiny.en.bin",
            WhisperModel::Base => "ggml-base.bin",
            WhisperModel::BaseEn => "ggml-base.en.bin",
            WhisperModel::Small => "ggml-small.bin",
            WhisperModel::SmallEn => "ggml-small.en.bin",
            WhisperModel::Medium => "ggml-medium.bin",
            WhisperModel::MediumEn => "ggml-medium.en.bin",
            WhisperModel::LargeV1 => "ggml-large-v1.bin",
            WhisperModel::LargeV2 => "ggml-large-v2.bin",
            WhisperModel::LargeV3 => "ggml-large-v3.bin",
            WhisperModel::LargeV3Turbo => "ggml-large-v3-turbo.bin",
        }
    }

    /// Get short name for settings storage
    pub fn short_name(&self) -> &'static str {
        match self {
            WhisperModel::Tiny => "tiny",
            WhisperModel::TinyEn => "tiny.en",
            WhisperModel::Base => "base",
            WhisperModel::BaseEn => "base.en",
            WhisperModel::Small => "small",
            WhisperModel::SmallEn => "small.en",
            WhisperModel::Medium => "medium",
            WhisperModel::MediumEn => "medium.en",
            WhisperModel::LargeV1 => "large-v1",
            WhisperModel::LargeV2 => "large-v2",
            WhisperModel::LargeV3 => "large-v3",
            WhisperModel::LargeV3Turbo => "large-v3-turbo",
        }
    }

    /// Parse from short name
    pub fn from_short_name(name: &str) -> Option<WhisperModel> {
        match name {
            "tiny" => Some(WhisperModel::Tiny),
            "tiny.en" => Some(WhisperModel::TinyEn),
            "base" => Some(WhisperModel::Base),
            "base.en" => Some(WhisperModel::BaseEn),
            "small" => Some(WhisperModel::Small),
            "small.en" => Some(WhisperModel::SmallEn),
            "medium" => Some(WhisperModel::Medium),
            "medium.en" => Some(WhisperModel::MediumEn),
            "large-v1" => Some(WhisperModel::LargeV1),
            "large-v2" => Some(WhisperModel::LargeV2),
            "large-v3" => Some(WhisperModel::LargeV3),
            "large-v3-turbo" => Some(WhisperModel::LargeV3Turbo),
            _ => None,
        }
    }

    /// Get approximate size in bytes
    pub fn size_bytes(&self) -> u64 {
        match self {
            WhisperModel::Tiny | WhisperModel::TinyEn => 75_000_000,
            WhisperModel::Base | WhisperModel::BaseEn => 142_000_000,
            WhisperModel::Small | WhisperModel::SmallEn => 466_000_000,
            WhisperModel::Medium | WhisperModel::MediumEn => 1_500_000_000,
            WhisperModel::LargeV1 | WhisperModel::LargeV2 | WhisperModel::LargeV3 => 2_900_000_000,
            WhisperModel::LargeV3Turbo => 1_600_000_000,
        }
    }
}

impl std::fmt::Display for WhisperModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// State of a model
#[derive(Debug, Clone, PartialEq)]
pub enum ModelState {
    /// Not downloaded
    NotDownloaded,
    /// Currently downloading
    Downloading { progress: f32 },
    /// Downloaded and ready
    Downloaded { path: PathBuf },
    /// Download failed
    Error { message: String },
}

/// Progress information for model download
#[derive(Debug, Clone)]
pub struct ModelDownloadProgress {
    /// Bytes downloaded so far
    pub downloaded_bytes: u64,
    /// Total bytes to download (if known)
    pub total_bytes: Option<u64>,
    /// Progress as fraction (0.0 - 1.0)
    pub progress: f32,
    /// Download speed in bytes per second
    pub speed_bytes_per_sec: u64,
    /// Whether download is complete
    pub is_complete: bool,
    /// Error message if failed
    pub error: Option<String>,
}

impl Default for ModelDownloadProgress {
    fn default() -> Self {
        Self {
            downloaded_bytes: 0,
            total_bytes: None,
            progress: 0.0,
            speed_bytes_per_sec: 0,
            is_complete: false,
            error: None,
        }
    }
}

/// Thread-safe progress tracker for downloads
#[derive(Clone)]
pub struct ProgressTracker {
    downloaded: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    is_complete: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
    cancelled: Arc<AtomicBool>,
}

impl ProgressTracker {
    pub fn new() -> Self {
        Self {
            downloaded: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(0)),
            is_complete: Arc::new(AtomicBool::new(false)),
            error: Arc::new(Mutex::new(None)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_total(&self, total: u64) {
        self.total.store(total, Ordering::SeqCst);
    }

    pub fn set_downloaded(&self, downloaded: u64) {
        self.downloaded.store(downloaded, Ordering::SeqCst);
    }

    pub fn set_complete(&self) {
        self.is_complete.store(true, Ordering::SeqCst);
    }

    pub fn set_error(&self, msg: String) {
        *self.error.lock().unwrap() = Some(msg);
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn get_progress(&self) -> ModelDownloadProgress {
        let downloaded = self.downloaded.load(Ordering::SeqCst);
        let total = self.total.load(Ordering::SeqCst);
        let is_complete = self.is_complete.load(Ordering::SeqCst);
        let error = self.error.lock().unwrap().clone();

        let progress = if total > 0 {
            downloaded as f32 / total as f32
        } else {
            0.0
        };

        ModelDownloadProgress {
            downloaded_bytes: downloaded,
            total_bytes: if total > 0 { Some(total) } else { None },
            progress,
            speed_bytes_per_sec: 0, // TODO: calculate actual speed
            is_complete,
            error,
        }
    }
}

impl Default for ProgressTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapper to implement hf-hub's Progress trait for ProgressTracker
#[derive(Clone)]
pub struct ProgressReporter {
    tracker: ProgressTracker,
}

impl ProgressReporter {
    pub fn new(tracker: ProgressTracker) -> Self {
        Self { tracker }
    }
}

impl Progress for ProgressReporter {
    async fn init(&mut self, size: usize, _filename: &str) {
        self.tracker.set_total(size as u64);
    }

    async fn update(&mut self, size: usize) {
        let current = self.tracker.downloaded.load(Ordering::SeqCst);
        self.tracker.set_downloaded(current + size as u64);
    }

    async fn finish(&mut self) {
        self.tracker.set_complete();
    }
}

/// Manager for Whisper models
pub struct ModelManager {
    /// Local cache directory for models
    cache_dir: PathBuf,
    /// HuggingFace repo containing GGML models
    repo_id: String,
}

impl ModelManager {
    /// Create a new model manager
    pub fn new() -> Result<Self, String> {
        // Use standard HuggingFace cache location
        let cache = Cache::default();
        let cache_dir = cache.path().to_path_buf();

        Ok(Self {
            cache_dir,
            // Using ggerganov's whisper.cpp repo which has GGML models
            repo_id: "ggerganov/whisper.cpp".to_string(),
        })
    }

    /// Get the cache directory path
    pub fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
    }

    /// Get the repo cache directory (where snapshots are stored)
    fn repo_cache_dir(&self) -> PathBuf {
        // hf-hub stores files in: cache_dir/models--{org}--{repo}/
        let repo_folder = self.repo_id.replace('/', "--");
        self.cache_dir.join(format!("models--{}", repo_folder))
    }

    /// Check if a model is downloaded locally by scanning the cache
    /// This does NOT trigger a download - it only checks existing files
    pub fn is_model_downloaded(&self, model: WhisperModel) -> bool {
        self.get_cached_model_path(model).is_some()
    }

    /// Get path to a cached model file, if it exists
    /// This scans the cache directory without triggering downloads
    pub fn get_cached_model_path(&self, model: WhisperModel) -> Option<PathBuf> {
        let repo_dir = self.repo_cache_dir();
        let snapshots_dir = repo_dir.join("snapshots");

        // Check if snapshots directory exists
        if !snapshots_dir.exists() {
            return None;
        }

        // Scan all snapshot directories for the model file
        let file_name = model.file_name();
        if let Ok(entries) = fs::read_dir(&snapshots_dir) {
            for entry in entries.flatten() {
                let snapshot_path = entry.path();
                if snapshot_path.is_dir() {
                    let model_path = snapshot_path.join(file_name);
                    if model_path.exists() && model_path.is_file() {
                        // Verify it's a real file, not a symlink to nowhere
                        if let Ok(metadata) = fs::metadata(&model_path) {
                            if metadata.len() > 0 {
                                return Some(model_path);
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Get state of a model
    pub fn get_model_state(&self, model: WhisperModel) -> ModelState {
        if let Some(path) = self.get_cached_model_path(model) {
            ModelState::Downloaded { path }
        } else {
            ModelState::NotDownloaded
        }
    }

    /// Get list of all models with their states
    pub fn list_models(&self) -> Vec<(WhisperModel, ModelState)> {
        WhisperModel::all()
            .iter()
            .map(|&model| (model, self.get_model_state(model)))
            .collect()
    }

    /// Get list of downloaded models
    pub fn list_downloaded_models(&self) -> Vec<WhisperModel> {
        WhisperModel::all()
            .iter()
            .filter(|&&model| self.is_model_downloaded(model))
            .copied()
            .collect()
    }

    /// Download a model with progress tracking (async)
    /// This is a static method that doesn't require holding the manager lock
    pub async fn download_model_with_progress(
        model: WhisperModel,
        cache_dir: PathBuf,
        repo_id: String,
        progress: ProgressTracker,
    ) -> Result<PathBuf, String> {
        // Check for cancellation
        if progress.is_cancelled() {
            return Err("Download cancelled".to_string());
        }

        // Create async API client
        let api = ApiBuilder::new()
            .with_cache_dir(cache_dir)
            .build()
            .map_err(|e| format!("Failed to create HuggingFace API: {}", e))?;

        let repo = api.model(repo_id);

        // Create progress reporter
        let reporter = ProgressReporter::new(progress.clone());

        // Download with progress tracking
        let result = repo
            .download_with_progress(model.file_name(), reporter)
            .await
            .map_err(|e| format!("Failed to download model {}: {}", model.display_name(), e));

        match &result {
            Ok(_) => {
                progress.set_complete();
            }
            Err(e) => {
                progress.set_error(e.clone());
            }
        }

        result
    }

    /// Delete a downloaded model
    pub fn delete_model(&self, model: WhisperModel) -> Result<(), String> {
        if let Some(path) = self.get_cached_model_path(model) {
            fs::remove_file(&path).map_err(|e| format!("Failed to delete model: {}", e))?;

            // Also try to remove the parent directory if empty
            if let Some(parent) = path.parent() {
                let _ = fs::remove_dir(parent);
            }
        }
        Ok(())
    }

    /// Delete all downloaded models
    pub fn delete_all_models(&self) -> Result<(), String> {
        for model in WhisperModel::all() {
            self.delete_model(*model)?;
        }
        Ok(())
    }
}

impl Default for ModelManager {
    fn default() -> Self {
        Self::new().expect("Failed to create ModelManager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_names() {
        assert_eq!(WhisperModel::Tiny.short_name(), "tiny");
        assert_eq!(
            WhisperModel::from_short_name("tiny"),
            Some(WhisperModel::Tiny)
        );
        assert_eq!(WhisperModel::from_short_name("invalid"), None);
    }

    #[test]
    fn test_model_file_names() {
        assert_eq!(WhisperModel::Tiny.file_name(), "ggml-tiny.bin");
        assert_eq!(WhisperModel::LargeV3.file_name(), "ggml-large-v3.bin");
    }
}
