//! Main application component for Adlib

use crate::audio::{
    AudioCapture, AudioPlayer, CaptureState, SharedCaptureState, SharedPlaybackState, WavRecorder,
};
use crate::models::{RecordingInfo, Segment, Transcription, TranscriptionStatus};
use crate::state::{ActiveView, AppState, RecordingsDatabase};
use crate::transcription::{resample, LiveTranscriber, TranscriptionEngine, TranscriptionOptions};
use crate::whisper::{ModelManager, ProgressTracker, WhisperModel};
use gpui::prelude::*;
use gpui::{InteractiveElement, *};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The root application view
pub struct Adlib {
    state: AppState,
    database: RecordingsDatabase,
    audio_capture: AudioCapture,
    capture_state: SharedCaptureState,
    audio_player: AudioPlayer,
    playback_state: SharedPlaybackState,
    /// Currently loaded recording path for playback
    loaded_recording_path: Option<PathBuf>,
    /// Error message from last load attempt
    load_error: Option<String>,
    /// Model manager for Whisper models
    model_manager: Arc<Mutex<ModelManager>>,
    /// Currently downloading model with progress tracker
    active_download: Option<(WhisperModel, ProgressTracker)>,
    /// Queue of models waiting to download
    download_queue: Vec<WhisperModel>,
    /// Last download error (for UI feedback)
    download_error: Option<String>,
    /// Currently transcribing file (if any)
    transcribing_file: Option<String>,
    /// Transcription status message
    transcription_status: Option<String>,
    _ui_refresh_task: Option<Task<()>>,
    // Live transcription state
    /// Live transcriber instance (loaded when entering Live mode)
    live_transcriber: Option<Arc<Mutex<LiveTranscriber>>>,
    /// Accumulated live transcript text
    live_transcript: String,
    /// Is live transcription currently running
    live_is_running: bool,
    /// Audio capture specifically for live mode (separate from recording)
    live_audio_capture: Option<AudioCapture>,
    /// Shared capture state for live mode
    live_capture_state: Option<SharedCaptureState>,
    /// Live duration in seconds
    live_duration: f64,
    /// Live transcription error (if any)
    live_error: Option<String>,
}

impl Adlib {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        let mut state = AppState::new();
        let database = RecordingsDatabase::new();

        // Load recordings from database (creates with demos on first run)
        match database.load() {
            Ok(recordings) => {
                state.recordings = recordings;
            }
            Err(e) => {
                eprintln!("Failed to load recordings database: {}", e);
            }
        }

        let audio_capture = AudioCapture::new();
        let capture_state = audio_capture.shared_state();
        let audio_player = AudioPlayer::new();
        let playback_state = audio_player.shared_state();

        // Initialize model manager
        let model_manager = match ModelManager::new() {
            Ok(mm) => Arc::new(Mutex::new(mm)),
            Err(e) => {
                eprintln!("Failed to create model manager: {}", e);
                Arc::new(Mutex::new(ModelManager::default()))
            }
        };

        Self {
            state,
            database,
            audio_capture,
            capture_state,
            audio_player,
            playback_state,
            loaded_recording_path: None,
            load_error: None,
            model_manager,
            active_download: None,
            download_queue: Vec::new(),
            download_error: None,
            transcribing_file: None,
            transcription_status: None,
            _ui_refresh_task: None,
            // Live transcription state
            live_transcriber: None,
            live_transcript: String::new(),
            live_is_running: false,
            live_audio_capture: None,
            live_capture_state: None,
            live_duration: 0.0,
            live_error: None,
        }
    }

    /// Start audio recording with UI refresh
    fn start_audio_capture(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = self.audio_capture.start() {
            eprintln!("Failed to start audio capture: {}", e);
            return;
        }

        // Spawn a task to refresh UI during recording
        let capture_state = self.capture_state.clone();
        self._ui_refresh_task = Some(cx.spawn({
            async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                loop {
                    // Check if still capturing
                    if capture_state.state() != CaptureState::Capturing {
                        break;
                    }

                    // Wait ~60fps refresh rate
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;

                    // Upgrade weak reference and notify to refresh the UI
                    let Some(this) = this.upgrade() else {
                        break;
                    };
                    let result = cx.update_entity(&this, |_, cx| {
                        cx.notify();
                    });
                    if result.is_err() {
                        break;
                    }
                }
            }
        }));
    }

    /// Stop audio recording and save to file
    fn stop_audio_capture(&mut self) -> Option<std::path::PathBuf> {
        // Get the actual sample rate before stopping (it resets on stop)
        let sample_rate = self.capture_state.sample_rate();

        match self.audio_capture.stop() {
            Ok(samples) => {
                if samples.is_empty() {
                    return None;
                }
                // Use the actual capture sample rate for the WAV file
                let recorder = WavRecorder::new().with_sample_rate(sample_rate);
                match recorder.save(&samples, None) {
                    Ok(path) => {
                        println!(
                            "Recording saved to: {:?} ({}Hz, {} samples)",
                            path,
                            sample_rate,
                            samples.len()
                        );
                        Some(path)
                    }
                    Err(e) => {
                        eprintln!("Failed to save recording: {}", e);
                        None
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to stop audio capture: {}", e);
                None
            }
        }
    }

    /// Get the path for a recording file
    fn recording_path(&self, file_name: &str) -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("adlib")
            .join("recordings")
            .join(file_name)
    }

    /// Check if a recording file exists
    fn recording_exists(&self, file_name: &str) -> bool {
        self.recording_path(file_name).exists()
    }

    /// Load a recording for playback
    fn load_recording(&mut self, file_name: &str) -> Result<(), String> {
        let path = self.recording_path(file_name);

        // Check if file exists first
        if !path.exists() {
            let err = format!("File not found: {}", file_name);
            self.load_error = Some(err.clone());
            return Err(err);
        }

        // Load the WAV file
        let (samples, sample_rate) = WavRecorder::load(&path).map_err(|e| {
            let err = format!("{} (path: {:?})", e, path);
            self.load_error = Some(err.clone());
            err
        })?;

        // Load into the player
        self.audio_player.load(samples, sample_rate);
        self.loaded_recording_path = Some(path);
        self.load_error = None;

        Ok(())
    }

    /// Start playback with UI refresh
    fn start_playback(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = self.audio_player.play() {
            eprintln!("Failed to start playback: {}", e);
            return;
        }

        // Spawn a task to refresh UI during playback
        let playback_state = self.playback_state.clone();
        self._ui_refresh_task = Some(cx.spawn({
            async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                loop {
                    // Check if still playing
                    if !playback_state.is_playing() {
                        break;
                    }

                    // Wait ~60fps refresh rate
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;

                    // Upgrade weak reference and notify to refresh the UI
                    let Some(this) = this.upgrade() else {
                        break;
                    };
                    let result = cx.update_entity(&this, |_, cx| {
                        cx.notify();
                    });
                    if result.is_err() {
                        break;
                    }
                }
            }
        }));
    }

    /// Stop playback
    fn stop_playback(&mut self) {
        self.audio_player.stop();
    }

    /// Toggle playback (play/pause)
    fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        if self.playback_state.is_playing() {
            self.stop_playback();
        } else {
            self.start_playback(cx);
        }
    }

    /// Save current recordings to the database
    fn save_recordings_to_db(&self) {
        if let Err(e) = self.database.save(&self.state.recordings) {
            eprintln!("Failed to save recordings database: {}", e);
        }
    }

    /// Add a new recording and save to database
    #[allow(dead_code)]
    fn add_recording(&mut self, recording: RecordingInfo) {
        self.state.recordings.insert(0, recording);
        self.save_recordings_to_db();
    }

    /// Queue a model for download
    fn queue_model_download(&mut self, model: WhisperModel, cx: &mut Context<Self>) {
        // Don't queue if already downloaded
        if self.is_model_downloaded(model) {
            return;
        }

        // Don't queue if already in queue or actively downloading
        if self.active_download.as_ref().map(|(m, _)| *m) == Some(model) {
            return;
        }
        if self.download_queue.contains(&model) {
            return;
        }

        self.download_queue.push(model);
        self.download_error = None;

        // Start download if nothing is active
        if self.active_download.is_none() {
            self.process_download_queue(cx);
        }
    }

    /// Process the next item in the download queue
    fn process_download_queue(&mut self, cx: &mut Context<Self>) {
        // Don't start if already downloading
        if self.active_download.is_some() {
            return;
        }

        // Get next model from queue
        let Some(model) = self.download_queue.first().copied() else {
            return;
        };
        self.download_queue.remove(0);

        let progress = ProgressTracker::new();
        self.active_download = Some((model, progress.clone()));
        self.download_error = None;

        // Get cache_dir and repo_id from manager (quick lock, then release)
        let (cache_dir, repo_id) = {
            let manager = self.model_manager.lock().unwrap();
            (
                manager.cache_dir().clone(),
                "ggerganov/whisper.cpp".to_string(),
            )
        };

        // Spawn download task - does NOT hold the mutex lock
        cx.spawn({
            let progress = progress.clone();
            async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                // Run the download in a background thread
                // Uses static method - no mutex needed!
                let result = cx
                    .background_executor()
                    .spawn({
                        let progress = progress.clone();
                        async move {
                            crate::whisper::ModelManager::download_model_with_progress(
                                model, cache_dir, repo_id, progress,
                            )
                        }
                    })
                    .await;

                // Update UI when done and process next in queue
                if let Some(this) = this.upgrade() {
                    let _ = cx.update_entity(&this, |this, cx| {
                        this.active_download = None;

                        if let Err(e) = result {
                            this.download_error = Some(format!(
                                "Failed to download {}: {}",
                                model.display_name(),
                                e
                            ));
                        }

                        // Process next in queue
                        this.process_download_queue(cx);
                        cx.notify();
                    });
                }
            }
        })
        .detach();

        // Start UI refresh for progress
        self.start_download_progress_refresh(cx);
    }

    /// Start UI refresh task for download progress
    fn start_download_progress_refresh(&mut self, cx: &mut Context<Self>) {
        self._ui_refresh_task = Some(cx.spawn({
            async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                loop {
                    // Wait before next refresh
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;

                    // Check if still downloading
                    let Some(this_ref) = this.upgrade() else {
                        break;
                    };

                    let should_continue = cx.update_entity(&this_ref, |this, cx| {
                        let still_downloading = this.active_download.is_some();
                        cx.notify();
                        still_downloading
                    });

                    match should_continue {
                        Ok(true) => continue,
                        _ => break,
                    }
                }
            }
        }));
    }

    /// Cancel the current download
    fn cancel_download(&mut self, cx: &mut Context<Self>) {
        if let Some((model, progress)) = self.active_download.take() {
            progress.cancel();
            self.download_error = Some(format!("{} download cancelled", model.display_name()));
        }
        // Process next in queue
        self.process_download_queue(cx);
    }

    /// Check if a model is downloaded
    fn is_model_downloaded(&self, model: WhisperModel) -> bool {
        let manager = self.model_manager.lock().unwrap();
        manager.is_model_downloaded(model)
    }

    /// Check if a model is queued for download
    fn is_model_queued(&self, model: WhisperModel) -> bool {
        self.download_queue.contains(&model)
    }

    /// Check if a model is actively downloading
    fn is_model_downloading(&self, model: WhisperModel) -> bool {
        self.active_download.as_ref().map(|(m, _)| *m) == Some(model)
    }

    /// Get download progress for active download (0.0 - 1.0)
    fn get_download_progress(&self) -> f32 {
        self.active_download
            .as_ref()
            .map(|(_, p)| p.get_progress().progress)
            .unwrap_or(0.0)
    }

    /// Select a model (only if downloaded)
    fn select_model(&mut self, model: WhisperModel) {
        if self.is_model_downloaded(model) {
            self.state.settings.selected_model_name = model.short_name().to_string();
        }
    }

    /// Delete a downloaded model
    fn delete_model(&mut self, model: WhisperModel) {
        let manager = self.model_manager.lock().unwrap();
        if let Err(e) = manager.delete_model(model) {
            self.download_error = Some(format!("Failed to delete {}: {}", model.display_name(), e));
        } else {
            // Reset selection if we deleted the selected model
            if self.state.settings.selected_model_name == model.short_name() {
                self.state.settings.selected_model_name = String::new();
            }
        }
    }

    /// Delete all downloaded models
    fn delete_all_models(&mut self) {
        let manager = self.model_manager.lock().unwrap();
        if let Err(e) = manager.delete_all_models() {
            self.download_error = Some(format!("Failed to delete models: {}", e));
        } else {
            self.state.settings.selected_model_name = String::new();
        }
    }

    /// Start transcribing a recording
    fn start_transcription(&mut self, file_name: &str, cx: &mut Context<Self>) {
        // Don't start if already transcribing
        if self.transcribing_file.is_some() {
            return;
        }

        // Get the selected model
        let selected_model_name = self.state.settings.selected_model_name.clone();
        if selected_model_name.is_empty() {
            self.transcription_status = Some(
                "No model selected. Go to Settings to download and select a model.".to_string(),
            );
            return;
        }

        // Find the model and check if it's downloaded
        let model = WhisperModel::recommended()
            .iter()
            .find(|m| m.short_name() == selected_model_name)
            .copied();

        let Some(model) = model else {
            self.transcription_status = Some("Selected model not found".to_string());
            return;
        };

        // Get the model path
        let model_path = {
            let manager = self.model_manager.lock().unwrap();
            manager.get_cached_model_path(model)
        };

        let Some(model_path) = model_path else {
            self.transcription_status =
                Some(format!("Model {} is not downloaded", model.display_name()));
            return;
        };

        // Get the recording path
        let recordings_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("adlib")
            .join("recordings");
        let wav_path = recordings_dir.join(file_name);

        if !wav_path.exists() {
            self.transcription_status = Some("Recording file not found".to_string());
            return;
        }

        self.transcribing_file = Some(file_name.to_string());
        self.transcription_status = Some("Loading model...".to_string());

        let file_name_clone = file_name.to_string();

        // Spawn transcription task
        cx.spawn({
            async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                // Update status to transcribing
                if let Some(this) = this.upgrade() {
                    let _ = cx.update_entity(&this, |this, cx| {
                        this.transcription_status = Some("Transcribing...".to_string());
                        cx.notify();
                    });
                }

                // Run transcription in background thread
                let result = cx
                    .background_executor()
                    .spawn({
                        let model_path = model_path.clone();
                        let wav_path = wav_path.clone();
                        async move {
                            // Load the model
                            let engine = TranscriptionEngine::new(&model_path)?;

                            // Transcribe the file
                            let options = TranscriptionOptions::default();
                            engine.transcribe_file(&wav_path, &options)
                        }
                    })
                    .await;

                // Update UI with result
                if let Some(this) = this.upgrade() {
                    let _ = cx.update_entity(&this, |this, cx| {
                        this.transcribing_file = None;

                        match result {
                            Ok(transcription_result) => {
                                this.transcription_status =
                                    Some("Transcription complete!".to_string());

                                // Update the recording with transcription
                                if let Some(recording) =
                                    this.state.get_recording_mut(&file_name_clone)
                                {
                                    let mut transcription = Transcription::new(
                                        file_name_clone.clone(),
                                        model.display_name().to_string(),
                                        Default::default(),
                                    );
                                    transcription.text = transcription_result.text;
                                    transcription.status = TranscriptionStatus::Done;

                                    // Store timestamped segments for karaoke-style display
                                    transcription.segments = transcription_result
                                        .segments
                                        .into_iter()
                                        .map(|seg| Segment {
                                            start_ms: (seg.start * 1000.0) as i64,
                                            end_ms: (seg.end * 1000.0) as i64,
                                            text: seg.text,
                                            tokens: Vec::new(),
                                            speaker: None,
                                            words: Vec::new(),
                                        })
                                        .collect();

                                    recording.transcription = Some(transcription);
                                }

                                // Save to database
                                if let Err(e) = this.database.save(&this.state.recordings) {
                                    eprintln!("Failed to save transcription: {}", e);
                                }
                            }
                            Err(e) => {
                                this.transcription_status =
                                    Some(format!("Transcription failed: {}", e));
                            }
                        }

                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }
}

impl Render for Adlib {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_view = self.state.active_view.clone();
        let show_help = self.state.show_help;
        let is_live = matches!(active_view, ActiveView::Live);
        let is_record = matches!(active_view, ActiveView::Record);
        let is_list = matches!(active_view, ActiveView::RecordingList);
        let is_settings = matches!(active_view, ActiveView::Settings);

        // Download status for sidebar
        let has_active_download = self.active_download.is_some();
        let download_model_name = self.active_download.as_ref().map(|(m, _)| m.display_name());
        let download_progress = self.get_download_progress();
        let queue_count = self.download_queue.len();
        let download_error = self.download_error.clone();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x0f0f1a))
            .key_context("Adlib")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, _cx| {
                match event.keystroke.key.as_str() {
                    "f1" => {
                        this.state.toggle_help();
                    }
                    "escape" => {
                        if this.state.show_help {
                            this.state.toggle_help();
                        } else if this.state.record_screen.is_recording {
                            this.state.cancel_recording();
                        }
                    }
                    "space" if !this.state.show_help => {
                        if this.state.record_screen.is_recording {
                            let saved_path = this.stop_audio_capture();
                            let file_name = saved_path.and_then(|p| {
                                p.file_name().map(|f| f.to_string_lossy().to_string())
                            });
                            this.state.stop_recording(file_name);
                            this.save_recordings_to_db();
                        } else {
                            this.state.start_recording();
                            this.start_audio_capture(_cx);
                        }
                    }
                    "1" if event.keystroke.modifiers.control => {
                        this.state.navigate_to(ActiveView::Record);
                    }
                    "2" if event.keystroke.modifiers.control => {
                        this.state.navigate_to(ActiveView::RecordingList);
                    }
                    "3" if event.keystroke.modifiers.control => {
                        this.state.navigate_to(ActiveView::Settings);
                    }
                    "q" if event.keystroke.modifiers.control => {
                        // If recording, save first before closing
                        if this.state.record_screen.is_recording {
                            let saved_path = this.stop_audio_capture();
                            let file_name = saved_path.and_then(|p| {
                                p.file_name().map(|f| f.to_string_lossy().to_string())
                            });
                            this.state.stop_recording(file_name);
                            this.save_recordings_to_db();
                        }
                        window.remove_window();
                    }
                    _ => {}
                }
            }))
            // Custom titlebar
            .child(
                div()
                    .id("titlebar")
                    .flex()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .h(px(36.0))
                    .bg(rgb(0x12121f))
                    .border_b_1()
                    .border_color(rgb(0x2d2d44))
                    .child(
                        // Window title (left side) - draggable area
                        div()
                            .id("titlebar-drag-area")
                            .flex()
                            .flex_grow()
                            .items_center()
                            .h_full()
                            .gap_2()
                            .px_4()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|_this, _event: &MouseDownEvent, window, _cx| {
                                    window.start_window_move();
                                }),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xcccccc))
                                    .child("Adlib - Voice Recorder"),
                            ),
                    )
                    .child(
                        // Close button (right side) - NOT draggable
                        div()
                            .id("close-button")
                            .w(px(46.0))
                            .h(px(36.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0xe81123)))
                            .on_click(cx.listener(|this, _, window, _cx| {
                                // If recording, save first before closing
                                if this.state.record_screen.is_recording {
                                    let saved_path = this.stop_audio_capture();
                                    let file_name = saved_path.and_then(|p| {
                                        p.file_name().map(|f| f.to_string_lossy().to_string())
                                    });
                                    this.state.stop_recording(file_name);
                                    this.save_recordings_to_db();
                                }
                                window.remove_window();
                            }))
                            .child(div().text_lg().text_color(rgb(0xcccccc)).child("×")),
                    ),
            )
            // Main content area (sidebar + content)
            .child(
                div()
                    .flex()
                    .flex_grow()
                    .overflow_hidden()
                    // Sidebar
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(px(200.0))
                            .h_full()
                            .bg(rgb(0x1a1a2e))
                            .border_r_1()
                            .border_color(rgb(0x2d2d44))
                            .child(
                                // App title
                                div()
                                    .px_4()
                                    .py_3()
                                    .border_b_1()
                                    .border_color(rgb(0x2d2d44))
                                    .child(
                                        div()
                                            .text_xl()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(0xe94560))
                                            .child("Adlib"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0x888888))
                                            .child("Voice Recorder"),
                                    ),
                            )
                            .child(
                                // Navigation items
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .p_2()
                                    .flex_grow()
                                    .child(
                                        div()
                                            .id("nav-live")
                                            .px_3()
                                            .py_2()
                                            .rounded_md()
                                            .bg(if is_live {
                                                rgb(0x2d2d44)
                                            } else {
                                                rgb(0x1a1a2e)
                                            })
                                            .text_color(if is_live {
                                                rgb(0xe94560)
                                            } else {
                                                rgb(0xcccccc)
                                            })
                                            .cursor_pointer()
                                            .hover(|style| style.bg(rgb(0x2d2d44)))
                                            .on_click(cx.listener(|this, _, _w, _cx| {
                                                this.state.navigate_to(ActiveView::Live);
                                            }))
                                            .child("Live"),
                                    )
                                    .child(
                                        div()
                                            .id("nav-record")
                                            .px_3()
                                            .py_2()
                                            .rounded_md()
                                            .bg(if is_record {
                                                rgb(0x2d2d44)
                                            } else {
                                                rgb(0x1a1a2e)
                                            })
                                            .text_color(if is_record {
                                                rgb(0xe94560)
                                            } else {
                                                rgb(0xcccccc)
                                            })
                                            .cursor_pointer()
                                            .hover(|style| style.bg(rgb(0x2d2d44)))
                                            .on_click(cx.listener(|this, _, _w, _cx| {
                                                this.state.navigate_to(ActiveView::Record);
                                            }))
                                            .child("Record"),
                                    )
                                    .child(
                                        div()
                                            .id("nav-recordings")
                                            .px_3()
                                            .py_2()
                                            .rounded_md()
                                            .bg(if is_list {
                                                rgb(0x2d2d44)
                                            } else {
                                                rgb(0x1a1a2e)
                                            })
                                            .text_color(if is_list {
                                                rgb(0xe94560)
                                            } else {
                                                rgb(0xcccccc)
                                            })
                                            .cursor_pointer()
                                            .hover(|style| style.bg(rgb(0x2d2d44)))
                                            .on_click(cx.listener(|this, _, _w, _cx| {
                                                this.state.navigate_to(ActiveView::RecordingList);
                                            }))
                                            .child("Recordings"),
                                    )
                                    .child(
                                        div()
                                            .id("nav-settings")
                                            .px_3()
                                            .py_2()
                                            .rounded_md()
                                            .bg(if is_settings {
                                                rgb(0x2d2d44)
                                            } else {
                                                rgb(0x1a1a2e)
                                            })
                                            .text_color(if is_settings {
                                                rgb(0xe94560)
                                            } else {
                                                rgb(0xcccccc)
                                            })
                                            .cursor_pointer()
                                            .hover(|style| style.bg(rgb(0x2d2d44)))
                                            .on_click(cx.listener(|this, _, _w, _cx| {
                                                this.state.navigate_to(ActiveView::Settings);
                                            }))
                                            .child("Settings"),
                                    ),
                            )
                            // Download status (when active)
                            .when(has_active_download || download_error.is_some(), |el| {
                                el.child(
                                    div()
                                        .px_3()
                                        .py_2()
                                        .border_t_1()
                                        .border_color(rgb(0x2d2d44))
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        // Error message
                                        .when(download_error.is_some(), |el| {
                                            let err = download_error.clone().unwrap_or_default();
                                            el.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(0xf44336))
                                                    .child(err),
                                            )
                                        })
                                        // Active download
                                        .when(has_active_download, |el| {
                                            let model_name = download_model_name.unwrap_or("Model");
                                            let progress_pct = (download_progress * 100.0) as u32;
                                            el.child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .justify_between()
                                                            .items_center()
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(rgb(0xcccccc))
                                                                    .child(format!(
                                                                        "Downloading {}",
                                                                        model_name
                                                                    )),
                                                            )
                                                            .child(
                                                                div()
                                                                    .id("cancel-download")
                                                                    .text_xs()
                                                                    .text_color(rgb(0xf44336))
                                                                    .cursor_pointer()
                                                                    .hover(|s| {
                                                                        s.text_color(rgb(0xff6666))
                                                                    })
                                                                    .on_click(cx.listener(
                                                                        |this, _, _w, cx| {
                                                                            this.cancel_download(
                                                                                cx,
                                                                            );
                                                                        },
                                                                    ))
                                                                    .child("Cancel"),
                                                            ),
                                                    )
                                                    .child(
                                                        // Progress bar
                                                        div()
                                                            .w_full()
                                                            .h(px(4.0))
                                                            .bg(rgb(0x2d2d44))
                                                            .rounded_full()
                                                            .child(
                                                                div()
                                                                    .h_full()
                                                                    .rounded_full()
                                                                    .bg(rgb(0xFF9800))
                                                                    .w(relative(download_progress)),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(rgb(0x888888))
                                                            .child(if queue_count > 0 {
                                                                format!(
                                                                    "{}% ({} queued)",
                                                                    progress_pct, queue_count
                                                                )
                                                            } else {
                                                                format!("{}%", progress_pct)
                                                            }),
                                                    ),
                                            )
                                        }),
                                )
                            })
                            .child(
                                // Help hint at bottom
                                div()
                                    .px_4()
                                    .py_3()
                                    .border_t_1()
                                    .border_color(rgb(0x2d2d44))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0x666666))
                                            .child("Press F1 for help"),
                                    ),
                            ),
                    )
                    // Main content area
                    .child(
                        div()
                            .flex_grow()
                            .h_full()
                            .relative()
                            .child(match &active_view {
                                ActiveView::Live => self.render_live_view(cx).into_any_element(),
                                ActiveView::Record => {
                                    self.render_record_view(cx).into_any_element()
                                }
                                ActiveView::RecordingList => {
                                    self.render_recording_list(cx).into_any_element()
                                }
                                ActiveView::RecordingDetails(id) => {
                                    let id = id.clone();
                                    self.render_recording_details(&id, cx).into_any_element()
                                }
                                ActiveView::Settings => self.render_settings(cx).into_any_element(),
                            })
                            .when(show_help, |el| el.child(render_help_overlay())),
                    ),
            )
    }
}

impl Adlib {
    /// Start live transcription
    fn start_live_transcription(&mut self, cx: &mut Context<Self>) {
        // Check if a model is available
        let model_path = {
            let manager = self.model_manager.lock().unwrap();
            // Try to find any downloaded model, preferring the selected one
            let selected = WhisperModel::from_short_name(&self.state.settings.selected_model_name)
                .unwrap_or(WhisperModel::Tiny);
            if let Some(path) = manager.get_cached_model_path(selected) {
                Some(path)
            } else {
                // Try to find any downloaded model
                WhisperModel::all()
                    .iter()
                    .find_map(|&m| manager.get_cached_model_path(m))
            }
        };

        let Some(model_path) = model_path else {
            self.live_error =
                Some("No model downloaded. Go to Settings to download a model.".to_string());
            return;
        };

        // Create the live transcriber
        match LiveTranscriber::new(&model_path) {
            Ok(transcriber) => {
                self.live_transcriber = Some(Arc::new(Mutex::new(transcriber)));
                self.live_error = None;
            }
            Err(e) => {
                self.live_error = Some(format!("Failed to load model: {}", e));
                return;
            }
        }

        // Create a new audio capture for live mode
        let mut live_capture = AudioCapture::new();
        let live_state = live_capture.shared_state();

        if let Err(e) = live_capture.start() {
            self.live_error = Some(format!("Failed to start audio: {}", e));
            self.live_transcriber = None;
            return;
        }

        self.live_capture_state = Some(live_state.clone());
        self.live_audio_capture = Some(live_capture);
        self.live_is_running = true;
        self.live_duration = 0.0;
        self.live_transcript.clear();

        // Start UI refresh task for smooth waveform (60fps like Record mode)
        let ui_capture_state = live_state.clone();
        self._ui_refresh_task = Some(cx.spawn({
            async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                loop {
                    // Check if still running
                    let should_stop = this
                        .update(cx, |this, _| !this.live_is_running)
                        .unwrap_or(true);

                    if should_stop || ui_capture_state.state() != CaptureState::Capturing {
                        break;
                    }

                    // Wait ~60fps refresh rate
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;

                    // Notify to refresh the UI (waveform)
                    let Some(this) = this.upgrade() else {
                        break;
                    };
                    let result = cx.update_entity(&this, |_, cx| {
                        cx.notify();
                    });
                    if result.is_err() {
                        break;
                    }
                }
            }
        }));

        // Start a task to process audio and update transcription
        let transcriber = self.live_transcriber.clone().unwrap();
        let capture_state = live_state;

        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut last_sample_count = 0usize;

            loop {
                // Sleep for a bit to accumulate audio
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;

                // Check if we should stop
                let should_stop = this
                    .update(cx, |this, _| !this.live_is_running)
                    .unwrap_or(true);

                if should_stop {
                    break;
                }

                // Get new samples from capture
                let samples = capture_state.samples();
                let duration = capture_state.duration();

                // Update duration
                let _ = this.update(cx, |this, _| {
                    this.live_duration = duration;
                });

                // Check if we have new samples to process
                if samples.len() > last_sample_count {
                    let new_samples = &samples[last_sample_count..];
                    last_sample_count = samples.len();

                    // Get the sample rate from capture and resample to 16kHz if needed
                    let sample_rate = capture_state.sample_rate();
                    let samples_16k = if sample_rate != 16000 {
                        // PipeWire typically captures at 48kHz - resample to 16kHz for Whisper
                        resample(new_samples, sample_rate, 16000)
                    } else {
                        new_samples.to_vec()
                    };

                    // Add resampled samples to transcriber
                    {
                        let mut t = transcriber.lock().unwrap();
                        t.add_samples(&samples_16k);
                    }

                    // Check if ready to process
                    let ready = {
                        let t = transcriber.lock().unwrap();
                        t.ready_to_process()
                    };

                    if ready {
                        // Process Whisper on a background thread to avoid blocking UI
                        let transcriber_clone = transcriber.clone();
                        let (result, full_transcript) = cx
                            .background_executor()
                            .spawn(async move {
                                let mut t = transcriber_clone.lock().unwrap();
                                let result = t.process();
                                let transcript = t.get_transcript();
                                (result, transcript)
                            })
                            .await;

                        match result {
                            Ok(true) => {
                                let _ = this.update(cx, |this, cx| {
                                    this.live_transcript = full_transcript;
                                    cx.notify();
                                });
                            }
                            Ok(false) => {
                                let _ = this.update(cx, |this, cx| {
                                    if this.live_transcript != full_transcript {
                                        this.live_transcript = full_transcript;
                                        cx.notify();
                                    }
                                });
                            }
                            Err(e) => {
                                let _ = this.update(cx, |this, cx| {
                                    this.live_error = Some(format!("Transcription error: {}", e));
                                    cx.notify();
                                });
                            }
                        }
                    }
                }

                // Notify UI to update
                let _ = this.update(cx, |_, cx| {
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Stop live transcription
    fn stop_live_transcription(&mut self) {
        self.live_is_running = false;

        // Stop audio capture
        if let Some(mut capture) = self.live_audio_capture.take() {
            let _ = capture.stop();
        }

        self.live_capture_state = None;
        // Keep transcriber and transcript for viewing/copying
    }

    /// Clear live transcript
    fn clear_live_transcript(&mut self) {
        self.live_transcript.clear();
        if let Some(transcriber) = &self.live_transcriber {
            let mut t = transcriber.lock().unwrap();
            t.clear();
        }
        self.live_duration = 0.0;
    }

    /// Copy live transcript to clipboard and primary selection (X11)
    fn copy_live_transcript(&self, cx: &mut Context<Self>) {
        if !self.live_transcript.is_empty() {
            let item = ClipboardItem::new_string(self.live_transcript.clone());
            cx.write_to_clipboard(item.clone());
            cx.write_to_primary(item);
        }
    }

    fn render_live_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_running = self.live_is_running;
        let transcript = self.live_transcript.clone();
        let duration = self.live_duration;
        let error = self.live_error.clone();

        // Get waveform from live capture if running
        let waveform_samples = self
            .live_capture_state
            .as_ref()
            .map(|s| s.waveform_samples())
            .unwrap_or_default();
        let _volume_level = self
            .live_capture_state
            .as_ref()
            .map(|s| s.volume_level())
            .unwrap_or(0.0);

        // Get calibration status
        let (is_calibrating, calibration_progress) = self
            .live_transcriber
            .as_ref()
            .map(|t| {
                let t = t.lock().unwrap();
                (!t.is_calibrated(), t.calibration_progress())
            })
            .unwrap_or((false, 0.0));

        // Check if a model is available
        let has_model = {
            let manager = self.model_manager.lock().unwrap();
            WhisperModel::all()
                .iter()
                .any(|&m| manager.is_model_downloaded(m))
        };

        let format_duration = |secs: f64| {
            let total_seconds = secs as u64;
            let minutes = total_seconds / 60;
            let seconds = total_seconds % 60;
            format!("{:02}:{:02}", minutes, seconds)
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x16213e))
            .child(
                // Header
                div()
                    .px_6()
                    .py_4()
                    .border_b_1()
                    .border_color(rgb(0x2d2d44))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0xffffff))
                            .child(if is_calibrating && is_running {
                                "Calibrating..."
                            } else if is_running {
                                "Live Transcription..."
                            } else {
                                "Live Transcription"
                            }),
                    )
                    // Calibration progress bar
                    .when(is_calibrating && is_running, |el| {
                        el.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0xffa500))
                                        .child("Stay quiet..."),
                                )
                                .child(
                                    div()
                                        .w(px(100.0))
                                        .h(px(8.0))
                                        .bg(rgb(0x2d2d44))
                                        .rounded_full()
                                        .child(
                                            div()
                                                .h_full()
                                                .rounded_full()
                                                .bg(rgb(0xffa500))
                                                .w(relative(calibration_progress)),
                                        ),
                                ),
                        )
                    }),
            )
            // Error message
            .when(error.is_some(), |el| {
                let err = error.clone().unwrap_or_default();
                el.child(
                    div()
                        .px_6()
                        .py_2()
                        .bg(rgb(0x4a1c1c))
                        .text_color(rgb(0xf44336))
                        .text_sm()
                        .child(err),
                )
            })
            // No model warning
            .when(!has_model && !is_running, |el| {
                el.child(
                    div()
                        .px_6()
                        .py_4()
                        .child(
                            div()
                                .p_4()
                                .bg(rgb(0x2d2d44))
                                .rounded_lg()
                                .text_color(rgb(0xffa500))
                                .text_sm()
                                .child("No model downloaded. Go to Settings to download a Whisper model first."),
                        ),
                )
            })
            // Waveform display
            .child(
                div()
                    .px_6()
                    .py_4()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w(px(400.0))
                            .h(px(100.0))
                            .bg(rgb(0x1a1a2e))
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(0x2d2d44))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(!is_running, |el| {
                                el.child(
                                    div()
                                        .text_color(rgb(0x666666))
                                        .text_sm()
                                        .child("Press Start to begin live transcription"),
                                )
                            })
                            .when(is_running, |el| {
                                // Show waveform bars - fixed 48 bars like Record view
                                let num_bars = 48usize;
                                let num_samples = waveform_samples.len();

                                el.child(
                                    div()
                                        .flex()
                                        .items_end()
                                        .justify_center()
                                        .gap_1()
                                        .h(px(60.0))
                                        .children((0..num_bars).map(move |i| {
                                            let height = if num_samples > 0 {
                                                // Calculate which bars have data (fill from right)
                                                let bars_with_data = num_samples.min(num_bars);
                                                let first_bar_with_data = num_bars - bars_with_data;

                                                if i >= first_bar_with_data {
                                                    // This bar has data
                                                    let samples_to_skip = num_samples.saturating_sub(num_bars);
                                                    let bar_offset = i - first_bar_with_data;
                                                    let sample_idx = samples_to_skip + bar_offset;
                                                    let sample = waveform_samples.get(sample_idx).copied().unwrap_or(0.0);
                                                    (sample * 200.0).clamp(2.0, 60.0)
                                                } else {
                                                    // No data yet - minimal height
                                                    2.0
                                                }
                                            } else {
                                                2.0
                                            };
                                            div()
                                                .w(px(4.0))
                                                .h(px(height))
                                                .bg(rgb(0xe94560))
                                                .rounded_sm()
                                        })),
                                )
                            }),
                    ),
            )
            // Transcript area
            .child(
                div()
                    .id("live-transcript-scroll")
                    .flex_grow()
                    .px_6()
                    .py_4()
                    .overflow_y_scroll()
                    .overflow_x_hidden()
                    .child(
                        div()
                            .p_4()
                            .bg(rgb(0x1a1a2e))
                            .rounded_lg()
                            .min_h(px(200.0))
                            .w_full()
                            .overflow_hidden()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(0x888888))
                                    .mb_2()
                                    .child("Transcript"),
                            )
                            .child(
                                div()
                                    .text_base()
                                    .text_color(rgb(0xcccccc))
                                    .child(if transcript.is_empty() {
                                        if is_running {
                                            "Listening...".to_string()
                                        } else {
                                            "Transcript will appear here".to_string()
                                        }
                                    } else {
                                        // Insert newlines at word boundaries for wrapping
                                        // (~10 words per line for readable text)
                                        let words: Vec<&str> = transcript.split_whitespace().collect();
                                        let mut lines = Vec::new();
                                        let mut current_line = Vec::new();
                                        for word in words {
                                            current_line.push(word);
                                            if current_line.len() >= 10 {
                                                lines.push(current_line.join(" "));
                                                current_line = Vec::new();
                                            }
                                        }
                                        if !current_line.is_empty() {
                                            lines.push(current_line.join(" "));
                                        }
                                        lines.join("\n")
                                    }),
                            ),
                    ),
            )
            // Controls
            .child(
                div()
                    .px_6()
                    .py_4()
                    .border_t_1()
                    .border_color(rgb(0x2d2d44))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        // Buttons
                        div()
                            .flex()
                            .gap_3()
                            // Start/Stop button
                            .child(
                                div()
                                    .id("live-toggle")
                                    .px_6()
                                    .py_2()
                                    .rounded_lg()
                                    .cursor_pointer()
                                    .bg(if is_running { rgb(0xf44336) } else { rgb(0x4caf50) })
                                    .hover(|s| s.bg(if is_running { rgb(0xd32f2f) } else { rgb(0x45a049) }))
                                    .text_color(rgb(0xffffff))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .when(!has_model && !is_running, |el| {
                                        el.opacity(0.5).cursor_default()
                                    })
                                    .on_click(cx.listener(move |this, _, _w, cx| {
                                        if this.live_is_running {
                                            this.stop_live_transcription();
                                        } else if has_model {
                                            this.start_live_transcription(cx);
                                        }
                                    }))
                                    .child(if is_running { "Stop" } else { "Start" }),
                            )
                            // Copy button
                            .child(
                                div()
                                    .id("live-copy")
                                    .px_4()
                                    .py_2()
                                    .rounded_lg()
                                    .cursor_pointer()
                                    .bg(rgb(0x2d2d44))
                                    .hover(|s| s.bg(rgb(0x3d3d54)))
                                    .text_color(rgb(0xcccccc))
                                    .when(transcript.is_empty(), |el| {
                                        el.opacity(0.5).cursor_default()
                                    })
                                    .on_click(cx.listener(|this, _, _w, cx| {
                                        this.copy_live_transcript(cx);
                                    }))
                                    .child("Copy"),
                            )
                            // Clear button
                            .child(
                                div()
                                    .id("live-clear")
                                    .px_4()
                                    .py_2()
                                    .rounded_lg()
                                    .cursor_pointer()
                                    .bg(rgb(0x2d2d44))
                                    .hover(|s| s.bg(rgb(0x3d3d54)))
                                    .text_color(rgb(0xcccccc))
                                    .when(transcript.is_empty() && !is_running, |el| {
                                        el.opacity(0.5).cursor_default()
                                    })
                                    .on_click(cx.listener(|this, _, _w, _cx| {
                                        this.stop_live_transcription();
                                        this.clear_live_transcript();
                                    }))
                                    .child("Clear"),
                            ),
                    )
                    // Duration display
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(if is_running { rgb(0xe94560) } else { rgb(0x666666) })
                            .child(format_duration(duration)),
                    ),
            )
    }

    fn render_record_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_recording = self.state.record_screen.is_recording;
        let is_paused = self.state.record_screen.is_paused;

        // Use live duration from audio capture when recording, otherwise use state
        let duration = if is_recording && !is_paused {
            self.capture_state.duration()
        } else {
            self.state.record_screen.duration_seconds
        };

        // Get live waveform samples from PipeWire capture
        let waveform_samples = self.capture_state.waveform_samples();
        let volume_level = self.capture_state.volume_level();

        let format_duration = |secs: f64| {
            let total_seconds = secs as u64;
            let minutes = total_seconds / 60;
            let seconds = total_seconds % 60;
            format!("{:02}:{:02}", minutes, seconds)
        };

        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .size_full()
            .bg(rgb(0x16213e))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_8()
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0xffffff))
                            .child(if is_recording {
                                if is_paused {
                                    "Recording Paused"
                                } else {
                                    "Recording..."
                                }
                            } else {
                                "Ready to Record"
                            }),
                    )
                    .child(
                        // Waveform / Volume meter display
                        div()
                            .w(px(400.0))
                            .h(px(120.0))
                            .bg(rgb(0x1a1a2e))
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(0x2d2d44))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .when(!is_recording, |el| {
                                el.child(
                                    div()
                                        .text_color(rgb(0x666666))
                                        .text_sm()
                                        .child("Audio waveform will appear here"),
                                )
                                .child(
                                    div()
                                        .text_color(rgb(0x444444))
                                        .text_xs()
                                        .child("Press Record or Space to start"),
                                )
                            })
                            .when(is_recording, |el| {
                                // Volume meter bars - driven by live PipeWire audio
                                // Discrete updates: bars shift left when new sample arrives
                                // Bars fill from right to left (newest on right)
                                let num_bars = 48usize;
                                let num_samples = waveform_samples.len();

                                el.child(
                                    div()
                                        .flex()
                                        .items_end()
                                        .justify_center()
                                        .gap_1()
                                        .h(px(60.0))
                                        .children((0..num_bars).map(|i| {
                                            let height = if is_paused {
                                                5.0
                                            } else if num_samples > 0 {
                                                // Calculate which bars have data (fill from right)
                                                let bars_with_data = num_samples.min(num_bars);
                                                let first_bar_with_data = num_bars - bars_with_data;

                                                if i >= first_bar_with_data {
                                                    // This bar has data
                                                    let samples_to_skip =
                                                        num_samples.saturating_sub(num_bars);
                                                    let bar_offset = i - first_bar_with_data;
                                                    let sample_idx = samples_to_skip + bar_offset;
                                                    let sample = waveform_samples
                                                        .get(sample_idx)
                                                        .copied()
                                                        .unwrap_or(0.0);
                                                    (sample * 200.0).clamp(5.0, 60.0)
                                                } else {
                                                    // No data yet - minimal height
                                                    5.0
                                                }
                                            } else {
                                                (volume_level * 200.0).clamp(5.0, 60.0)
                                            };
                                            div().w(px(4.0)).h(px(height)).rounded_sm().bg(
                                                if is_paused {
                                                    rgb(0x444444)
                                                } else if height > 54.0 {
                                                    rgb(0xe94560)
                                                } else if height > 35.0 {
                                                    rgb(0xFF9800)
                                                } else {
                                                    rgb(0x4CAF50)
                                                },
                                            )
                                        })),
                                )
                                .child(
                                    div()
                                        .text_color(rgb(0x888888))
                                        .text_xs()
                                        .child(if is_paused {
                                            "Paused - Click Resume to continue"
                                        } else {
                                            "Recording... Speak into your microphone"
                                        }),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_3xl()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if is_recording {
                                rgb(0xe94560)
                            } else {
                                rgb(0x888888)
                            })
                            .child(format_duration(duration)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_4()
                            .when(!is_recording, |el| {
                                el.child(
                                    div()
                                        .id("btn-record")
                                        .px_6()
                                        .py_3()
                                        .rounded_lg()
                                        .bg(rgb(0xe94560))
                                        .text_color(rgb(0xffffff))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .cursor_pointer()
                                        .hover(|style| style.opacity(0.9))
                                        .on_click(cx.listener(|this, _, _w, cx| {
                                            this.state.start_recording();
                                            this.start_audio_capture(cx);
                                        }))
                                        .child("Record"),
                                )
                            })
                            .when(is_recording && is_paused, |el| {
                                el.child(
                                    div()
                                        .id("btn-resume")
                                        .px_6()
                                        .py_3()
                                        .rounded_lg()
                                        .bg(rgb(0x4CAF50))
                                        .text_color(rgb(0xffffff))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .cursor_pointer()
                                        .hover(|style| style.opacity(0.9))
                                        .on_click(cx.listener(|this, _, _w, _cx| {
                                            this.state.resume_recording();
                                        }))
                                        .child("Resume"),
                                )
                            })
                            .when(is_recording && !is_paused, |el| {
                                el.child(
                                    div()
                                        .id("btn-pause")
                                        .px_6()
                                        .py_3()
                                        .rounded_lg()
                                        .bg(rgb(0xFF9800))
                                        .text_color(rgb(0xffffff))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .cursor_pointer()
                                        .hover(|style| style.opacity(0.9))
                                        .on_click(cx.listener(|this, _, _w, _cx| {
                                            this.state.pause_recording();
                                        }))
                                        .child("Pause"),
                                )
                            })
                            .when(is_recording, |el| {
                                el.child(
                                    div()
                                        .id("btn-stop")
                                        .px_6()
                                        .py_3()
                                        .rounded_lg()
                                        .bg(rgb(0x4CAF50))
                                        .text_color(rgb(0xffffff))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .cursor_pointer()
                                        .hover(|style| style.opacity(0.9))
                                        .on_click(cx.listener(|this, _, _w, _cx| {
                                            let saved_path = this.stop_audio_capture();
                                            let file_name = saved_path.and_then(|p| {
                                                p.file_name()
                                                    .map(|f| f.to_string_lossy().to_string())
                                            });
                                            this.state.stop_recording(file_name);
                                            this.save_recordings_to_db();
                                        }))
                                        .child("Stop & Save"),
                                )
                                .child(
                                    div()
                                        .id("btn-cancel")
                                        .px_6()
                                        .py_3()
                                        .rounded_lg()
                                        .bg(rgb(0x666666))
                                        .text_color(rgb(0xffffff))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .cursor_pointer()
                                        .hover(|style| style.opacity(0.9))
                                        .on_click(cx.listener(|this, _, _w, _cx| {
                                            this.state.cancel_recording();
                                            let _ = this.audio_capture.stop();
                                        }))
                                        .child("Cancel"),
                                )
                            }),
                    )
                    .child(div().text_sm().text_color(rgb(0x888888)).mt_8().child(
                        if is_recording {
                            "Recording audio from your microphone"
                        } else {
                            "Click Record or press Space to start recording"
                        },
                    )),
            )
    }

    fn render_recording_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let format_duration = |seconds: f64| {
            let total_seconds = seconds as u64;
            let minutes = total_seconds / 60;
            let secs = total_seconds % 60;
            format!("{}:{:02}", minutes, secs)
        };

        let format_date =
            |date: &chrono::DateTime<chrono::Utc>| date.format("%b %d, %Y %H:%M").to_string();

        let recordings: Vec<_> = self.state.recordings.clone();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x16213e))
            .child(
                div()
                    .px_6()
                    .py_4()
                    .border_b_1()
                    .border_color(rgb(0x2d2d44))
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0xffffff))
                            .child("Recordings"),
                    )
                    .child(
                        div()
                            .id("import-btn")
                            .px_4()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(0x2d2d44))
                            .text_color(rgb(0xcccccc))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x3d3d54)))
                            .child("Import Audio"),
                    ),
            )
            .child(
                div()
                    .id("recording-list-scroll")
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_4()
                    .flex_grow()
                    .overflow_y_scroll()
                    .when(recordings.is_empty(), |el| {
                        el.child(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .flex_grow()
                                .gap_4()
                                .child(
                                    div()
                                        .text_2xl()
                                        .text_color(rgb(0x2d2d44))
                                        .child("No recordings yet"),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0x666666))
                                        .child("Start recording or import audio files"),
                                ),
                        )
                    })
                    .when(!recordings.is_empty(), |el| {
                        el.children(recordings.iter().enumerate().map(|(idx, recording)| {
                            let has_transcription = recording.transcription.is_some()
                                || recording.edited_text.is_some();
                            let text_preview = if !recording.text().is_empty() {
                                let text = recording.text();
                                if text.len() > 100 {
                                    format!("{}...", &text[..100])
                                } else {
                                    text.to_string()
                                }
                            } else {
                                "No transcription".to_string()
                            };
                            let file_name = recording.file_name.clone();
                            let title = recording.title.clone();
                            let date_str = format_date(&recording.date);
                            let duration_str = format_duration(recording.duration_seconds);

                            div()
                                .id(SharedString::from(format!("recording-{}", idx)))
                                .px_4()
                                .py_3()
                                .bg(rgb(0x1a1a2e))
                                .rounded_lg()
                                .border_1()
                                .border_color(rgb(0x2d2d44))
                                .cursor_pointer()
                                .hover(|style| style.border_color(rgb(0xe94560)))
                                .on_click(cx.listener(move |this, _, _w, _cx| {
                                    this.state.navigate_to(ActiveView::RecordingDetails(
                                        file_name.clone(),
                                    ));
                                }))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .text_base()
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(rgb(0xffffff))
                                                        .child(title),
                                                )
                                                .when(has_transcription, |el| {
                                                    el.child(
                                                        div()
                                                            .px_2()
                                                            .rounded_sm()
                                                            .bg(rgb(0x4CAF50))
                                                            .text_xs()
                                                            .text_color(rgb(0xffffff))
                                                            .child("Transcribed"),
                                                    )
                                                }),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(0x888888))
                                                .child(format!("{} | {}", date_str, duration_str)),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(0x666666))
                                                .mt_2()
                                                .child(text_preview),
                                        ),
                                )
                        }))
                    }),
            )
    }

    fn render_recording_details(&mut self, id: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let format_duration = |seconds: f64| {
            let total_seconds = seconds as u64;
            let minutes = total_seconds / 60;
            let secs = total_seconds % 60;
            format!("{}:{:02}", minutes, secs)
        };

        let recording = self.state.get_recording(id).cloned();

        // Get playback state
        let is_playing = self.playback_state.is_playing();
        let current_time = self.playback_state.current_time();
        let progress = self.playback_state.progress();
        let waveform = self.playback_state.waveform();
        let file_name_for_load = id.to_string();

        match recording {
            None => div()
                .size_full()
                .bg(rgb(0x16213e))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_color(rgb(0x888888))
                        .child("Select a recording to view details"),
                ),
            Some(recording) => {
                let text = recording.text().to_string();
                let has_text = !text.is_empty();
                let duration = recording.duration_seconds;
                let duration_str = format_duration(duration);
                let current_time_str = format_duration(current_time);
                let title = recording.title.clone();
                let file_name = recording.file_name.clone();

                // Get segments for karaoke display
                let segments = recording
                    .transcription
                    .as_ref()
                    .map(|t| t.segments.clone())
                    .unwrap_or_default();
                let has_segments = !segments.is_empty();
                let current_time_ms = (current_time * 1000.0) as i64;

                // Check if the audio file exists
                let file_exists = self.recording_exists(&file_name);
                let load_error = self.load_error.clone();

                // Check if this recording is loaded
                let is_loaded = self
                    .loaded_recording_path
                    .as_ref()
                    .map(|p| {
                        p.file_name().map(|f| f.to_string_lossy().to_string())
                            == Some(file_name.clone())
                    })
                    .unwrap_or(false);

                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .bg(rgb(0x16213e))
                    .child(
                        div()
                            .px_6()
                            .py_4()
                            .border_b_1()
                            .border_color(rgb(0x2d2d44))
                            .flex()
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .id("back-btn")
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .bg(rgb(0x2d2d44))
                                    .text_color(rgb(0xcccccc))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(0x3d3d54)))
                                    .on_click(cx.listener(|this, _, _w, _cx| {
                                        this.stop_playback();
                                        this.state.navigate_to(ActiveView::RecordingList);
                                    }))
                                    .child("< Back"),
                            )
                            .child(
                                div()
                                    .flex_grow()
                                    .text_xl()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(0xffffff))
                                    .child(title),
                            ),
                    )
                    // Waveform and playback controls
                    .child(
                        div()
                            .px_6()
                            .py_4()
                            .border_b_1()
                            .border_color(rgb(0x2d2d44))
                            .bg(rgb(0x1a1a2e))
                            .flex()
                            .flex_col()
                            .gap_3()
                            // Waveform visualization
                            .child(
                                div()
                                    .flex()
                                    .items_end()
                                    .justify_center()
                                    .gap_px()
                                    .h(px(60.0))
                                    // File missing message
                                    .when(!file_exists, |el| {
                                        el.child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .items_center()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(rgb(0xf44336))
                                                        .child("Audio file not found"),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(rgb(0x666666))
                                                        .child(file_name.clone()),
                                                ),
                                        )
                                    })
                                    // Load error message
                                    .when(file_exists && load_error.is_some() && waveform.is_empty(), |el| {
                                        let error_msg = load_error.clone().unwrap_or_default();
                                        el.child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(0xf44336))
                                                .child(error_msg),
                                        )
                                    })
                                    // Ready to load message
                                    .when(file_exists && load_error.is_none() && waveform.is_empty(), |el| {
                                        el.child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(0x666666))
                                                .child("Click play to load waveform"),
                                        )
                                    })
                                    // Waveform bars
                                    .when(!waveform.is_empty(), |el| {
                                        let num_bars = waveform.len();
                                        let position_bar = (progress * num_bars as f32) as usize;
                                        el.children(waveform.iter().enumerate().map(|(i, &sample)| {
                                            let height = (sample * 200.0).clamp(3.0, 60.0);
                                            let is_played = i < position_bar;
                                            let is_current = i == position_bar;
                                            let color = if is_current {
                                                rgb(0xffffff)
                                            } else if is_played {
                                                rgb(0xe94560)
                                            } else {
                                                rgb(0x4a4a6a)
                                            };
                                            div()
                                                .w(px(3.0))
                                                .h(px(height))
                                                .rounded_sm()
                                                .bg(color)
                                        }))
                                    }),
                            )
                            // Playback controls row
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_4()
                                    .child(
                                        div()
                                            .id("play-btn")
                                            .w(px(40.0))
                                            .h(px(40.0))
                                            .rounded_full()
                                            .bg(if file_exists { rgb(0xe94560) } else { rgb(0x444444) })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .when(file_exists, |el| {
                                                el.cursor_pointer()
                                                    .hover(|style| style.opacity(0.9))
                                                    .on_click(cx.listener(move |this, _, _w, cx| {
                                                        // Load recording if not loaded
                                                        let file_to_load = file_name_for_load.clone();
                                                        let needs_load = !this.loaded_recording_path
                                                            .as_ref()
                                                            .map(|p| p.file_name().map(|f| f.to_string_lossy().to_string()) == Some(file_to_load.clone()))
                                                            .unwrap_or(false);

                                                        if needs_load {
                                                            if let Err(e) = this.load_recording(&file_to_load) {
                                                                eprintln!("Failed to load recording: {}", e);
                                                                cx.notify(); // Refresh UI to show error
                                                                return;
                                                            }
                                                        }

                                                        this.toggle_playback(cx);
                                                    }))
                                            })
                                            .child(
                                                div()
                                                    .text_color(if file_exists { rgb(0xffffff) } else { rgb(0x888888) })
                                                    .child(if is_playing && is_loaded { "||" } else { ">" }),
                                            ),
                                    )
                                    // Progress bar
                                    .child(
                                        div()
                                            .flex_grow()
                                            .h(px(8.0))
                                            .bg(rgb(0x2d2d44))
                                            .rounded_full()
                                            .relative()
                                            .child(
                                                div()
                                                    .absolute()
                                                    .left_0()
                                                    .top_0()
                                                    .h_full()
                                                    .rounded_full()
                                                    .bg(rgb(0xe94560))
                                                    .w(relative(progress)),
                                            ),
                                    )
                                    // Time display
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(0x888888))
                                            .min_w(px(80.0))
                                            .child(format!("{} / {}", current_time_str, duration_str)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("recording-details-scroll")
                            .flex()
                            .flex_col()
                            .flex_grow()
                            .p_6()
                            .overflow_y_scroll()
                            .when(!has_text, |el| {
                                el.child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .justify_center()
                                        .flex_grow()
                                        .gap_4()
                                        .child(
                                            div()
                                                .text_color(rgb(0x888888))
                                                .child("No transcription yet"),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(0x666666))
                                                .child(
                                                    "Click 'Transcribe' to generate text from audio",
                                                ),
                                        ),
                                )
                            })
                            // Karaoke-style segment display
                            .when(has_segments, |el| {
                                el.child(
                                    div()
                                        .flex()
                                        .flex_wrap()
                                        .gap_1()
                                        .children(segments.iter().enumerate().map(|(i, seg)| {
                                            let is_current = current_time_ms >= seg.start_ms && current_time_ms < seg.end_ms;
                                            let is_past = current_time_ms >= seg.end_ms;

                                            div()
                                                .id(SharedString::from(format!("seg-{}", i)))
                                                .px_1()
                                                .py_px()
                                                .rounded_sm()
                                                .text_base()
                                                .bg(if is_current { rgb(0xe94560) } else { rgb(0x1a1a2e) })
                                                .text_color(if is_current {
                                                    rgb(0xffffff)
                                                } else if is_past {
                                                    rgb(0xcccccc)
                                                } else {
                                                    rgb(0x666666)
                                                })
                                                .child(seg.text.clone())
                                        })),
                                )
                            })
                            // Fallback: plain text if we have text but no segments
                            .when(has_text && !has_segments, |el| {
                                el.child(div().text_base().text_color(rgb(0xcccccc)).child(text))
                            }),
                    )
                    .child({
                        let is_transcribing = self.transcribing_file.as_ref() == Some(&file_name);
                        let transcription_status = self.transcription_status.clone();
                        let file_name_for_transcribe = file_name.clone();

                        div()
                            .px_6()
                            .py_3()
                            .border_t_1()
                            .border_color(rgb(0x2d2d44))
                            .flex()
                            .flex_col()
                            .gap_2()
                            // Status message row
                            .when(transcription_status.is_some(), |el| {
                                let status = transcription_status.clone().unwrap_or_default();
                                el.child(
                                    div()
                                        .text_sm()
                                        .text_color(if status.contains("failed") || status.contains("not") {
                                            rgb(0xf44336)
                                        } else if status.contains("complete") {
                                            rgb(0x4CAF50)
                                        } else {
                                            rgb(0xFF9800)
                                        })
                                        .child(status),
                                )
                            })
                            // Buttons row
                            .child(
                                div()
                                    .flex()
                                    .gap_3()
                                    .child(
                                        div()
                                            .id("transcribe-btn")
                                            .px_4()
                                            .py_2()
                                            .rounded_md()
                                            .bg(if is_transcribing { rgb(0x666666) } else { rgb(0x4CAF50) })
                                            .text_sm()
                                            .text_color(rgb(0xffffff))
                                            .when(!is_transcribing, |el| {
                                                el.cursor_pointer()
                                                    .hover(|style| style.opacity(0.9))
                                                    .on_click(cx.listener(move |this, _, _w, cx| {
                                                        this.start_transcription(&file_name_for_transcribe, cx);
                                                    }))
                                            })
                                            .child(if is_transcribing { "Transcribing..." } else { "Transcribe" }),
                                    )
                                    .child(
                                        div()
                                            .id("export-btn")
                                            .px_4()
                                            .py_2()
                                            .rounded_md()
                                            .bg(rgb(0x2d2d44))
                                            .text_sm()
                                            .text_color(rgb(0xffffff))
                                            .cursor_pointer()
                                            .hover(|style| style.bg(rgb(0x3d3d54)))
                                            .child("Export Audio"),
                                    )
                                    .child(div().flex_grow())
                                    .child(
                                        div()
                                            .id("delete-btn")
                                            .px_4()
                                            .py_2()
                                            .rounded_md()
                                            .bg(rgb(0xf44336))
                                            .text_sm()
                                            .text_color(rgb(0xffffff))
                                            .cursor_pointer()
                                            .hover(|style| style.opacity(0.9))
                                            .child("Delete"),
                                    ),
                            )
                    })
            }
        }
    }

    /// Render a downloaded model row with Select and Delete buttons
    fn render_downloaded_model_row(
        &self,
        model: WhisperModel,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let model_name = model.display_name();
        let short_name = model.short_name();

        div()
            .id(SharedString::from(format!("model-dl-{}", short_name)))
            .flex()
            .items_center()
            .justify_between()
            .px_4()
            .py_3()
            .rounded_lg()
            .bg(if is_selected {
                rgb(0x2d2d44)
            } else {
                rgb(0x1a1a2e)
            })
            .border_1()
            .border_color(if is_selected {
                rgb(0xe94560)
            } else {
                rgb(0x2d2d44)
            })
            // Model name
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(0xffffff))
                    .child(model_name),
            )
            // Action buttons
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    // Select button (if not already selected)
                    .when(!is_selected, |el| {
                        el.child(
                            div()
                                .id(SharedString::from(format!("select-{}", short_name)))
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(rgb(0x4a9eff))
                                .text_xs()
                                .text_color(rgb(0xffffff))
                                .cursor_pointer()
                                .hover(|s| s.opacity(0.8))
                                .on_click(cx.listener(move |this, _, _w, cx| {
                                    this.select_model(model);
                                    cx.notify();
                                }))
                                .child("Select"),
                        )
                    })
                    // Selected indicator
                    .when(is_selected, |el| {
                        el.child(
                            div()
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(rgb(0xe94560))
                                .text_xs()
                                .text_color(rgb(0xffffff))
                                .child("Selected"),
                        )
                    })
                    // Delete button
                    .child(
                        div()
                            .id(SharedString::from(format!("delete-{}", short_name)))
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(rgb(0x2d2d44))
                            .text_xs()
                            .text_color(rgb(0xf44336))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x3d3d54)))
                            .on_click(cx.listener(move |this, _, _w, cx| {
                                this.delete_model(model);
                                cx.notify();
                            }))
                            .child("Delete"),
                    ),
            )
    }

    /// Render an available (not downloaded) model row with Download button
    fn render_available_model_row(
        &self,
        model: WhisperModel,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let model_name = model.display_name();
        let short_name = model.short_name();
        let is_downloading = self.is_model_downloading(model);
        let is_queued = self.is_model_queued(model);

        div()
            .id(SharedString::from(format!("model-av-{}", short_name)))
            .flex()
            .items_center()
            .justify_between()
            .px_4()
            .py_3()
            .rounded_lg()
            .bg(rgb(0x1a1a2e))
            .border_1()
            .border_color(rgb(0x2d2d44))
            // Model name
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(0x888888))
                    .child(model_name),
            )
            // Action button
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    // Downloading indicator
                    .when(is_downloading, |el| {
                        el.child(
                            div()
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(rgb(0xFF9800))
                                .text_xs()
                                .text_color(rgb(0xffffff))
                                .child("Downloading..."),
                        )
                    })
                    // Queued indicator
                    .when(is_queued && !is_downloading, |el| {
                        el.child(
                            div()
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(rgb(0x2d2d44))
                                .text_xs()
                                .text_color(rgb(0x888888))
                                .child("Queued"),
                        )
                    })
                    // Download button
                    .when(!is_downloading && !is_queued, |el| {
                        el.child(
                            div()
                                .id(SharedString::from(format!("download-{}", short_name)))
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(rgb(0x4CAF50))
                                .text_xs()
                                .text_color(rgb(0xffffff))
                                .cursor_pointer()
                                .hover(|s| s.opacity(0.8))
                                .on_click(cx.listener(move |this, _, _w, cx| {
                                    this.queue_model_download(model, cx);
                                    cx.notify();
                                }))
                                .child("Download"),
                        )
                    }),
            )
    }

    fn render_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_model = self.state.settings.selected_model_name.clone();
        let is_vad = self.state.settings.is_vad_enabled;
        let is_gpu = self.state.settings.is_using_gpu;
        let is_live = self.state.settings.is_live_transcription_enabled;
        let should_translate = self.state.settings.parameters.should_translate;
        let language = self.state.settings.parameters.language.clone();

        // Separate downloaded and available models
        let downloaded_models: Vec<(WhisperModel, bool)> = WhisperModel::recommended()
            .iter()
            .filter(|&&model| self.is_model_downloaded(model))
            .map(|&model| {
                let is_selected = model.short_name() == selected_model;
                (model, is_selected)
            })
            .collect();

        let available_models: Vec<WhisperModel> = WhisperModel::recommended()
            .iter()
            .filter(|&&model| !self.is_model_downloaded(model))
            .copied()
            .collect();

        let has_downloaded = !downloaded_models.is_empty();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x16213e))
            .child(
                div()
                    .px_6()
                    .py_4()
                    .border_b_1()
                    .border_color(rgb(0x2d2d44))
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(0xffffff))
                            .child("Settings"),
                    ),
            )
            .child(
                div()
                    .id("settings-scroll")
                    .flex()
                    .flex_col()
                    .gap_6()
                    .p_6()
                    .flex_grow()
                    .overflow_y_scroll()
                    // Downloaded Models Section
                    .when(has_downloaded, |el| {
                        el.child(settings_section(
                            "Downloaded Models",
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .children(downloaded_models.into_iter().map(
                                    |(model, is_selected)| {
                                        self.render_downloaded_model_row(model, is_selected, cx)
                                    },
                                ))
                                .child(
                                    // Delete All button
                                    div()
                                        .id("delete-all-models")
                                        .mt_2()
                                        .px_3()
                                        .py_2()
                                        .rounded_md()
                                        .bg(rgb(0x2d2d44))
                                        .text_xs()
                                        .text_color(rgb(0xf44336))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(rgb(0x3d3d54)))
                                        .on_click(cx.listener(|this, _, _w, cx| {
                                            this.delete_all_models();
                                            cx.notify();
                                        }))
                                        .child("Delete All Models"),
                                ),
                        ))
                    })
                    // Available Models Section
                    .child(settings_section(
                        "Available Models",
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .when(available_models.is_empty(), |el| {
                                el.child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0x888888))
                                        .child("All models downloaded"),
                                )
                            })
                            .when(!available_models.is_empty(), |el| {
                                el.children(
                                    available_models
                                        .into_iter()
                                        .map(|model| self.render_available_model_row(model, cx)),
                                )
                            })
                            .child(
                                div()
                                    .mt_2()
                                    .text_xs()
                                    .text_color(rgb(0x666666))
                                    .child("Larger models are more accurate but slower"),
                            ),
                    ))
                    // Transcription Options
                    .child(settings_section(
                        "Transcription Options",
                        div()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(setting_row(
                                "Language",
                                "Auto-detect or select specific",
                                language_dropdown(&language),
                            ))
                            .child(setting_row(
                                "Translate to English",
                                "Translate non-English audio",
                                toggle_switch(should_translate),
                            ))
                            .child(setting_row(
                                "Voice Activity Detection",
                                "Skip silent sections",
                                toggle_switch(is_vad),
                            )),
                    ))
                    // Performance
                    .child(settings_section(
                        "Performance",
                        div()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(setting_row(
                                "Use GPU Acceleration",
                                "Faster transcription if available",
                                toggle_switch(is_gpu),
                            ))
                            .child(setting_row(
                                "Live Transcription",
                                "Transcribe while recording",
                                toggle_switch(is_live),
                            )),
                    ))
                    // Storage
                    .child(settings_section(
                        "Storage",
                        div().flex().flex_col().gap_3().child(
                            div().flex().justify_between().items_center().child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_base()
                                            .text_color(rgb(0xcccccc))
                                            .child("Data Location"),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(0x888888))
                                            .child("~/.local/share/adlib/"),
                                    ),
                            ),
                        ),
                    ))
                    // About
                    .child(settings_section(
                        "About",
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .child(div().text_color(rgb(0x888888)).child("Version"))
                                    .child(div().text_color(rgb(0xcccccc)).child("0.1.0")),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .child(div().text_color(rgb(0x888888)).child("License"))
                                    .child(
                                        div().text_color(rgb(0xcccccc)).child("MIT / Apache-2.0"),
                                    ),
                            ),
                    )),
            )
    }
}

fn settings_section(title: &str, content: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0xe94560))
                .child(title.to_string()),
        )
        .child(
            div()
                .p_4()
                .rounded_lg()
                .bg(rgb(0x1a1a2e))
                .border_1()
                .border_color(rgb(0x2d2d44))
                .child(content),
        )
}

fn setting_row(label: &str, description: &str, control: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .justify_between()
        .items_center()
        .child(
            div()
                .flex()
                .flex_col()
                .flex_grow()
                .child(
                    div()
                        .text_base()
                        .text_color(rgb(0xcccccc))
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(0x666666))
                        .child(description.to_string()),
                ),
        )
        .child(control)
}

#[allow(dead_code)]
fn model_option(name: &str, size: &str, is_selected: bool) -> impl IntoElement {
    let bg = if is_selected {
        rgb(0xe94560)
    } else {
        rgb(0x2d2d44)
    };

    div()
        .id(SharedString::from(format!("model-{}", name)))
        .px_3()
        .py_2()
        .rounded_md()
        .bg(bg)
        .cursor_pointer()
        .hover(|style| style.opacity(0.9))
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(0xffffff))
                        .child(name.to_string()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(if is_selected {
                            rgb(0xffffff)
                        } else {
                            rgb(0x888888)
                        })
                        .child(size.to_string()),
                ),
        )
}

fn toggle_switch(is_on: bool) -> impl IntoElement {
    let bg = if is_on { rgb(0x4CAF50) } else { rgb(0x2d2d44) };
    let dot_position = if is_on { px(22.0) } else { px(2.0) };

    div()
        .w(px(44.0))
        .h(px(24.0))
        .rounded_full()
        .bg(bg)
        .cursor_pointer()
        .relative()
        .child(
            div()
                .absolute()
                .top(px(2.0))
                .left(dot_position)
                .w(px(20.0))
                .h(px(20.0))
                .rounded_full()
                .bg(rgb(0xffffff)),
        )
}

fn language_dropdown(current: &Option<String>) -> impl IntoElement {
    let display = current
        .as_ref()
        .map(|s| s.as_str())
        .unwrap_or("Auto-detect");

    div()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(rgb(0x2d2d44))
        .border_1()
        .border_color(rgb(0x3d3d54))
        .cursor_pointer()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(rgb(0xcccccc))
                .child(display.to_string()),
        )
        .child(div().text_xs().text_color(rgb(0x888888)).child("v"))
}

fn render_help_overlay() -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .bg(rgba(0x000000aa))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(600.0))
                .max_h(px(500.0))
                .bg(rgb(0x1a1a2e))
                .rounded_xl()
                .border_1()
                .border_color(rgb(0x2d2d44))
                .overflow_hidden()
                .flex()
                .flex_col()
                .child(
                    div()
                        .px_6()
                        .py_4()
                        .border_b_1()
                        .border_color(rgb(0x2d2d44))
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_xl()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0xffffff))
                                .child("Adlib Help"),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(0x888888))
                                .child("Press ESC or F1 to close"),
                        ),
                )
                .child(
                    div()
                        .id("help-scroll")
                        .p_6()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .flex_grow()
                        .overflow_y_scroll()
                        .child(help_section(
                            "Keyboard Shortcuts",
                            vec![
                                ("F1", "Toggle this help"),
                                ("Space", "Start/stop recording"),
                                ("Escape", "Cancel / Close"),
                                ("Ctrl+1", "Record view"),
                                ("Ctrl+2", "Recordings list"),
                                ("Ctrl+3", "Settings"),
                            ],
                        ))
                        .child(help_section(
                            "Recording",
                            vec![
                                ("Record", "Click or press Space"),
                                ("Pause/Resume", "Click while recording"),
                                ("Save", "Click 'Stop & Save'"),
                                ("Cancel", "Click Cancel or Escape"),
                            ],
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(
                                    div()
                                        .text_base()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(0xe94560))
                                        .child("Tips"),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0xcccccc))
                                        .child("- Use 'tiny' model for quick transcriptions"),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0xcccccc))
                                        .child("- Enable VAD to skip silence"),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0xcccccc))
                                        .child("- Recordings stored in ~/.local/share/adlib/"),
                                ),
                        ),
                ),
        )
}

fn help_section(title: &str, items: Vec<(&str, &str)>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_base()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0xe94560))
                .child(title.to_string()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .children(items.into_iter().map(|(key, desc)| {
                    div()
                        .flex()
                        .gap_4()
                        .child(
                            div()
                                .w(px(80.0))
                                .px_2()
                                .py_1()
                                .rounded_sm()
                                .bg(rgb(0x2d2d44))
                                .text_sm()
                                .text_color(rgb(0xe94560))
                                .child(key.to_string()),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(0xcccccc))
                                .child(desc.to_string()),
                        )
                })),
        )
}
