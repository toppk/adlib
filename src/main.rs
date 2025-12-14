//! Adlib - A voice recorder and transcription application for Linux
//!
//! This is the main entry point for the Adlib application.

mod app;
mod audio;
mod models;
mod state;

use app::Adlib;
use gpui::prelude::*;
use gpui::*;

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.0), px(800.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Adlib - Voice Recorder".into()),
                    appears_transparent: false,
                    ..Default::default()
                }),
                // Use server-side decorations for native window controls (close, minimize, etc.)
                window_decorations: Some(WindowDecorations::Server),
                // App ID for Wayland/GNOME desktop integration - matches .desktop file
                app_id: Some("com.adlib.VoiceRecorder".to_string()),
                ..Default::default()
            },
            |window, cx| {
                // Set app_id on the window for proper desktop integration
                window.set_app_id("com.adlib.VoiceRecorder");
                cx.new(Adlib::new)
            },
        )
        .expect("Failed to open window");
    });
}
