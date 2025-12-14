//! JSON-based database for persisting recordings
//!
//! Stores recording metadata in a JSON file at ~/.local/share/adlib/recordings.json

use crate::models::RecordingInfo;
use chrono::{Duration, Utc};
use std::fs;
use std::path::PathBuf;

/// Database for storing recording information
pub struct RecordingsDatabase {
    path: PathBuf,
}

impl RecordingsDatabase {
    /// Create a new database instance
    pub fn new() -> Self {
        let path = Self::default_path();
        Self { path }
    }

    /// Get the default database path
    fn default_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("adlib")
            .join("recordings.json")
    }

    /// Ensure the database directory exists
    fn ensure_dir(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create database directory: {}", e))?;
        }
        Ok(())
    }

    /// Load recordings from the database
    /// Creates the database with demo entries if it doesn't exist
    pub fn load(&self) -> Result<Vec<RecordingInfo>, String> {
        self.ensure_dir()?;

        if !self.path.exists() {
            // First run - create with demo recordings
            let demo_recordings = Self::create_demo_recordings();
            self.save(&demo_recordings)?;
            return Ok(demo_recordings);
        }

        let contents = fs::read_to_string(&self.path)
            .map_err(|e| format!("Failed to read database: {}", e))?;

        let recordings: Vec<RecordingInfo> = serde_json::from_str(&contents)
            .map_err(|e| format!("Failed to parse database: {}", e))?;

        Ok(recordings)
    }

    /// Save recordings to the database
    pub fn save(&self, recordings: &[RecordingInfo]) -> Result<(), String> {
        self.ensure_dir()?;

        let contents = serde_json::to_string_pretty(recordings)
            .map_err(|e| format!("Failed to serialize recordings: {}", e))?;

        fs::write(&self.path, contents)
            .map_err(|e| format!("Failed to write database: {}", e))?;

        Ok(())
    }

    /// Add a new recording and save to database
    pub fn add_recording(&self, recording: RecordingInfo, existing: &mut Vec<RecordingInfo>) -> Result<(), String> {
        existing.insert(0, recording);
        self.save(existing)
    }

    /// Delete a recording and save to database
    pub fn delete_recording(&self, file_name: &str, existing: &mut Vec<RecordingInfo>) -> Result<(), String> {
        existing.retain(|r| r.file_name != file_name);
        self.save(existing)
    }

    /// Create demo recordings for first run
    fn create_demo_recordings() -> Vec<RecordingInfo> {
        vec![
            RecordingInfo {
                file_name: "demo1.wav".to_string(),
                title: "Team Meeting Notes".to_string(),
                date: Utc::now(),
                duration_seconds: 125.5,
                edited_text: None,
                transcription: None,
            },
            RecordingInfo {
                file_name: "demo2.wav".to_string(),
                title: "Project Ideas".to_string(),
                date: Utc::now() - Duration::hours(2),
                duration_seconds: 45.2,
                edited_text: Some("This is a demo transcription text for the project ideas recording. It demonstrates how the text would appear in the details view.".to_string()),
                transcription: None,
            },
            RecordingInfo {
                file_name: "demo3.wav".to_string(),
                title: "Voice Memo".to_string(),
                date: Utc::now() - Duration::days(1),
                duration_seconds: 12.8,
                edited_text: None,
                transcription: None,
            },
        ]
    }
}

impl Default for RecordingsDatabase {
    fn default() -> Self {
        Self::new()
    }
}
