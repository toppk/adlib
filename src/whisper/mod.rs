//! Whisper model management and download
//!
//! Handles downloading Whisper GGML models from Hugging Face with progress tracking
//! and resume support.

mod manager;

pub use manager::{ModelManager, ModelState, ProgressTracker, WhisperModel};
