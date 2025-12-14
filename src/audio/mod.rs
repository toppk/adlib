//! Audio capture and processing module using PipeWire
//!
//! This module provides:
//! - Microphone capture at 16kHz mono (Whisper-compatible)
//! - Real-time volume metering
//! - WAV file recording via hound

mod capture;
mod recorder;

pub use capture::{AudioCapture, AudioDevice, CaptureState, SharedCaptureState};
pub use recorder::WavRecorder;
