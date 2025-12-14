//! Audio capture and playback module using PipeWire
//!
//! This module provides:
//! - Microphone capture at 16kHz mono (Whisper-compatible)
//! - Real-time volume metering
//! - WAV file recording via hound
//! - Audio playback with waveform visualization

mod capture;
mod playback;
mod recorder;

pub use capture::{AudioCapture, AudioDevice, CaptureState, SharedCaptureState};
pub use playback::{AudioPlayer, SharedPlaybackState};
pub use recorder::WavRecorder;
