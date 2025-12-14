//! Main application component for Adlib

use crate::audio::{AudioCapture, AudioPlayer, CaptureState, SharedCaptureState, SharedPlaybackState, WavRecorder};
use crate::models::RecordingInfo;
use crate::state::{ActiveView, AppState, RecordingsDatabase};
use gpui::prelude::*;
use gpui::{InteractiveElement, *};
use std::path::PathBuf;
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
    _ui_refresh_task: Option<Task<()>>,
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

        Self {
            state,
            database,
            audio_capture,
            capture_state,
            audio_player,
            playback_state,
            loaded_recording_path: None,
            load_error: None,
            _ui_refresh_task: None,
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
        let (samples, sample_rate) = WavRecorder::load(&path)
            .map_err(|e| {
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
    fn add_recording(&mut self, recording: RecordingInfo) {
        self.state.recordings.insert(0, recording);
        self.save_recordings_to_db();
    }
}

impl Render for Adlib {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_view = self.state.active_view.clone();
        let show_help = self.state.show_help;
        let is_record = matches!(active_view, ActiveView::Record);
        let is_list = matches!(active_view, ActiveView::RecordingList);
        let is_settings = matches!(active_view, ActiveView::Settings);

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
                            .child(
                                div()
                                    .text_lg()
                                    .text_color(rgb(0xcccccc))
                                    .child("×"),
                            ),
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
                                    .id("nav-record")
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .bg(if is_record { rgb(0x2d2d44) } else { rgb(0x1a1a2e) })
                                    .text_color(if is_record { rgb(0xe94560) } else { rgb(0xcccccc) })
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
                                    .bg(if is_list { rgb(0x2d2d44) } else { rgb(0x1a1a2e) })
                                    .text_color(if is_list { rgb(0xe94560) } else { rgb(0xcccccc) })
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
                                    .bg(if is_settings { rgb(0x2d2d44) } else { rgb(0x1a1a2e) })
                                    .text_color(if is_settings { rgb(0xe94560) } else { rgb(0xcccccc) })
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(0x2d2d44)))
                                    .on_click(cx.listener(|this, _, _w, _cx| {
                                        this.state.navigate_to(ActiveView::Settings);
                                    }))
                                    .child("Settings"),
                            ),
                    )
                    .child(
                        // Help hint at bottom
                        div()
                            .px_4()
                            .py_3()
                            .border_t_1()
                            .border_color(rgb(0x2d2d44))
                            .child(div().text_xs().text_color(rgb(0x666666)).child("Press F1 for help")),
                    ),
            )
            // Main content area
            .child(
                div()
                    .flex_grow()
                    .h_full()
                    .relative()
                    .child(match &active_view {
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
                        ActiveView::Settings => {
                            self.render_settings().into_any_element()
                        }
                    })
                    .when(show_help, |el| el.child(render_help_overlay())),
            ),
        )
    }
}

impl Adlib {
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
                                                    let samples_to_skip = num_samples.saturating_sub(num_bars);
                                                    let bar_offset = i - first_bar_with_data;
                                                    let sample_idx = samples_to_skip + bar_offset;
                                                    let sample = waveform_samples.get(sample_idx).copied().unwrap_or(0.0);
                                                    (sample * 200.0).clamp(5.0, 60.0)
                                                } else {
                                                    // No data yet - minimal height
                                                    5.0
                                                }
                                            } else {
                                                (volume_level * 200.0).clamp(5.0, 60.0)
                                            };
                                            div()
                                                .w(px(4.0))
                                                .h(px(height))
                                                .rounded_sm()
                                                .bg(if is_paused {
                                                    rgb(0x444444)
                                                } else if height > 54.0 {
                                                    rgb(0xe94560)
                                                } else if height > 35.0 {
                                                    rgb(0xFF9800)
                                                } else {
                                                    rgb(0x4CAF50)
                                                })
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
                                                p.file_name().map(|f| f.to_string_lossy().to_string())
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
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x888888))
                            .mt_8()
                            .child(if is_recording {
                                "Recording audio from your microphone"
                            } else {
                                "Click Record or press Space to start recording"
                            }),
                    ),
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
                                    this.state
                                        .navigate_to(ActiveView::RecordingDetails(file_name.clone()));
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

                // Check if the audio file exists
                let file_exists = self.recording_exists(&file_name);
                let load_error = self.load_error.clone();

                // Check if this recording is loaded
                let is_loaded = self.loaded_recording_path
                    .as_ref()
                    .map(|p| p.file_name().map(|f| f.to_string_lossy().to_string()) == Some(file_name.clone()))
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
                            .when(has_text, |el| {
                                el.child(div().text_base().text_color(rgb(0xcccccc)).child(text))
                            }),
                    )
                    .child(
                        div()
                            .px_6()
                            .py_3()
                            .border_t_1()
                            .border_color(rgb(0x2d2d44))
                            .flex()
                            .gap_3()
                            .child(
                                div()
                                    .id("transcribe-btn")
                                    .px_4()
                                    .py_2()
                                    .rounded_md()
                                    .bg(rgb(0x4CAF50))
                                    .text_sm()
                                    .text_color(rgb(0xffffff))
                                    .cursor_pointer()
                                    .hover(|style| style.opacity(0.9))
                                    .child("Transcribe"),
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
            }
        }
    }

    fn render_settings(&self) -> impl IntoElement {
        let selected_model = self.state.settings.selected_model_name.clone();
        let is_vad = self.state.settings.is_vad_enabled;
        let is_gpu = self.state.settings.is_using_gpu;
        let is_live = self.state.settings.is_live_transcription_enabled;
        let should_translate = self.state.settings.parameters.should_translate;
        let language = self.state.settings.parameters.language.clone();

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
                    // Model Selection
                    .child(settings_section(
                        "Whisper Model",
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x888888))
                                    .child("Selected Model"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(model_option("tiny", "~75MB", selected_model == "tiny"))
                                    .child(model_option("base", "~150MB", selected_model == "base"))
                                    .child(model_option(
                                        "small",
                                        "~500MB",
                                        selected_model == "small",
                                    ))
                                    .child(model_option(
                                        "medium",
                                        "~1.5GB",
                                        selected_model == "medium",
                                    ))
                                    .child(model_option(
                                        "large",
                                        "~3GB",
                                        selected_model == "large",
                                    )),
                            )
                            .child(
                                div()
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
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .items_center()
                                    .child(
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
                                    .child(div().text_color(rgb(0xcccccc)).child("MIT / Apache-2.0")),
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
    let display = current.as_ref().map(|s| s.as_str()).unwrap_or("Auto-detect");

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
