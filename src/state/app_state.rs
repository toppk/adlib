use crate::models::{RecordingInfo, Settings};
use uuid::Uuid;

/// The currently active view/screen
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ActiveView {
    #[default]
    Record,
    RecordingList,
    RecordingDetails(String), // recording file_name
    Settings,
}

/// State for recording screen
#[derive(Debug, Clone, Default)]
pub struct RecordScreenState {
    pub is_recording: bool,
    pub is_paused: bool,
    pub duration_seconds: f64,
    pub wave_samples: Vec<f32>,
    pub current_file: Option<String>,
}

/// State for playback controls
#[derive(Debug, Clone, Default)]
pub struct PlaybackState {
    pub is_playing: bool,
    pub current_time: f64,
    pub duration: f64,
    pub playback_rate: f32,
}

/// Root application state
#[derive(Debug, Clone)]
pub struct AppState {
    pub active_view: ActiveView,
    pub recordings: Vec<RecordingInfo>,
    pub settings: Settings,
    pub record_screen: RecordScreenState,
    pub playback: PlaybackState,
    pub selected_recording: Option<String>,
    pub show_help: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            active_view: ActiveView::Record,
            recordings: Vec::new(),
            settings: Settings::default(),
            record_screen: RecordScreenState::default(),
            playback: PlaybackState::default(),
            selected_recording: None,
            show_help: false,
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Navigate to a specific view
    pub fn navigate_to(&mut self, view: ActiveView) {
        self.active_view = view;
    }

    /// Start a new recording
    pub fn start_recording(&mut self) {
        let file_name = format!("{}.wav", Uuid::new_v4());
        self.record_screen = RecordScreenState {
            is_recording: true,
            is_paused: false,
            duration_seconds: 0.0,
            wave_samples: Vec::new(),
            current_file: Some(file_name),
        };
    }

    /// Pause the current recording
    pub fn pause_recording(&mut self) {
        if self.record_screen.is_recording {
            self.record_screen.is_paused = true;
        }
    }

    /// Resume the current recording
    pub fn resume_recording(&mut self) {
        if self.record_screen.is_recording {
            self.record_screen.is_paused = false;
        }
    }

    /// Stop and save the current recording
    /// If actual_file_name is provided, use it instead of the placeholder
    pub fn stop_recording(&mut self, actual_file_name: Option<String>) {
        let file_name = actual_file_name
            .or_else(|| self.record_screen.current_file.take())
            .unwrap_or_else(|| "unknown.wav".to_string());

        let mut recording = RecordingInfo::new(file_name);
        recording.duration_seconds = self.record_screen.duration_seconds;
        self.recordings.insert(0, recording);
        self.record_screen = RecordScreenState::default();
    }

    /// Cancel the current recording without saving
    pub fn cancel_recording(&mut self) {
        self.record_screen = RecordScreenState::default();
    }

    /// Delete a recording by file name
    pub fn delete_recording(&mut self, file_name: &str) {
        self.recordings.retain(|r| r.file_name != file_name);
        if self.selected_recording.as_deref() == Some(file_name) {
            self.selected_recording = None;
            self.active_view = ActiveView::RecordingList;
        }
    }

    /// Get a recording by file name
    pub fn get_recording(&self, file_name: &str) -> Option<&RecordingInfo> {
        self.recordings.iter().find(|r| r.file_name == file_name)
    }

    /// Get a mutable recording by file name
    pub fn get_recording_mut(&mut self, file_name: &str) -> Option<&mut RecordingInfo> {
        self.recordings.iter_mut().find(|r| r.file_name == file_name)
    }

    /// Toggle help overlay
    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }
}
