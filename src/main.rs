//! Adlib - A voice recorder and transcription application for Linux
//!
//! This is the main entry point for the Adlib application.

mod app;
mod assets;
mod audio;
mod cli;
mod models;
mod settings;
mod state;
mod tokio_runtime;
mod transcription;
mod whisper;

use app::Adlib;
use assets::Assets;
use clap::Parser;
use gpui::prelude::*;
use gpui::*;
use log::info;

fn main() {
    // Parse command-line arguments and initialize logging
    let args = cli::Args::parse();
    cli::init_logging(&args);

    // Route whisper.cpp logs through our logging system
    whisper::init_logging();

    info!("Starting Adlib voice recorder");

    Application::new().with_assets(Assets).run(|cx: &mut App| {
        // Initialize global Tokio runtime for hf-hub/reqwest async operations
        tokio_runtime::init(cx);
        let bounds = Bounds::centered(None, size(px(1200.0), px(800.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // No titlebar - we'll draw our own
                titlebar: None,
                // Use client-side decorations so we can draw our own titlebar
                window_decorations: Some(WindowDecorations::Client),
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
