#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Parameters for transcription configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TranscriptionParameters {
    pub initial_prompt: Option<String>,
    pub language: Option<String>,
    pub offset_ms: i64,
    pub should_translate: bool,
}

/// Timing information from transcription
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TranscriptionTimings {
    pub tokens_per_second: f64,
    pub full_pipeline_seconds: f64,
}

/// A single token from transcription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub id: i32,
    pub index: i32,
    pub log_probability: f64,
    pub speaker: Option<String>,
}

/// Word-level data from transcription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordData {
    pub word: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub probability: f64,
}

/// A segment of transcribed audio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub tokens: Vec<Token>,
    pub speaker: Option<String>,
    pub words: Vec<WordData>,
}

/// Status of a transcription job
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum TranscriptionStatus {
    #[default]
    NotStarted,
    Loading,
    Progress(f64), // progress fraction 0.0-1.0
    Done,
    Canceled,
    Error(String),
    Paused,
}

/// A transcription result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcription {
    pub id: Uuid,
    pub file_name: String,
    pub start_date: DateTime<Utc>,
    pub parameters: TranscriptionParameters,
    pub model_name: String,
    pub status: TranscriptionStatus,
    pub text: String,
    pub segments: Vec<Segment>,
    pub timings: TranscriptionTimings,
}

impl Transcription {
    pub fn new(file_name: String, model_name: String, parameters: TranscriptionParameters) -> Self {
        Self {
            id: Uuid::new_v4(),
            file_name,
            start_date: Utc::now(),
            parameters,
            model_name,
            status: TranscriptionStatus::NotStarted,
            text: String::new(),
            segments: Vec::new(),
            timings: TranscriptionTimings::default(),
        }
    }

    pub fn progress(&self) -> f64 {
        match &self.status {
            TranscriptionStatus::NotStarted => 0.0,
            TranscriptionStatus::Loading => 0.0,
            TranscriptionStatus::Progress(p) => *p,
            TranscriptionStatus::Done => 1.0,
            TranscriptionStatus::Canceled => 0.0,
            TranscriptionStatus::Error(_) => 0.0,
            TranscriptionStatus::Paused => 0.0,
        }
    }
}

/// Information about a recording
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingInfo {
    pub file_name: String,
    pub title: String,
    pub date: DateTime<Utc>,
    pub duration_seconds: f64,
    pub edited_text: Option<String>,
    pub transcription: Option<Transcription>,
}

impl RecordingInfo {
    pub fn new(file_name: String) -> Self {
        let now = Utc::now();
        Self {
            file_name,
            title: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            date: now,
            duration_seconds: 0.0,
            edited_text: None,
            transcription: None,
        }
    }

    pub fn id(&self) -> &str {
        &self.file_name
    }

    pub fn text(&self) -> &str {
        if let Some(edited) = &self.edited_text {
            edited
        } else if let Some(transcription) = &self.transcription {
            &transcription.text
        } else {
            ""
        }
    }
}

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub selected_model_name: String,
    pub parameters: TranscriptionParameters,
    pub is_using_gpu: bool,
    pub is_vad_enabled: bool,
    pub is_live_transcription_enabled: bool,
    pub confirm_on_delete: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            selected_model_name: "tiny".to_string(),
            parameters: TranscriptionParameters::default(),
            is_using_gpu: false,
            is_vad_enabled: false,
            is_live_transcription_enabled: false,
            confirm_on_delete: true,
        }
    }
}

/// Information about a Whisper model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub is_local: bool,
    pub is_default: bool,
    pub is_disabled: bool,
    pub size_bytes: Option<u64>,
}

/// A queued transcription task
#[derive(Debug, Clone)]
pub struct TranscriptionTask {
    pub id: Uuid,
    pub recording_info_id: String,
    pub settings: Settings,
}
