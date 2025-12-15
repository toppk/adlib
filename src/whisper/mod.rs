//! Whisper model management and download
//!
//! Handles downloading Whisper GGML models from Hugging Face with progress tracking
//! and resume support.

mod manager;

use log::{debug, error, info, trace, warn};
use whisper_rs::GGMLLogLevel;

pub use manager::{ModelManager, ProgressTracker, WhisperModel};

/// Custom log callback for whisper.cpp that routes output through our logging system
///
/// # Safety
/// This function is called from C code in whisper.cpp. The `text` pointer must be
/// a valid null-terminated C string.
unsafe extern "C" fn whisper_log_callback(
    level: u32,
    text: *const std::os::raw::c_char,
    _user_data: *mut std::os::raw::c_void,
) {
    if text.is_null() {
        error!("whisper_log_callback: text is nullptr");
        return;
    }

    // SAFETY: we trust whisper.cpp to pass valid C strings
    let log_str = unsafe { std::ffi::CStr::from_ptr(text) }.to_string_lossy();
    let trimmed = log_str.trim();

    // Skip empty messages
    if trimmed.is_empty() {
        return;
    }

    let level = GGMLLogLevel::from(level);

    match level {
        GGMLLogLevel::None => debug!(target: "whisper", "{}", trimmed),
        GGMLLogLevel::Debug => debug!(target: "whisper", "{}", trimmed),
        GGMLLogLevel::Info => info!(target: "whisper", "{}", trimmed),
        GGMLLogLevel::Warn => warn!(target: "whisper", "{}", trimmed),
        GGMLLogLevel::Error => error!(target: "whisper", "{}", trimmed),
        GGMLLogLevel::Cont => trace!(target: "whisper", "{}", trimmed),
        GGMLLogLevel::Unknown(lvl) => {
            warn!(target: "whisper", "unknown log level {}: {}", lvl, trimmed)
        }
    }
}

/// Initialize whisper logging to route through our logging system
pub fn init_logging() {
    // SAFETY: We're setting a valid function pointer as the callback
    unsafe {
        whisper_rs::set_log_callback(Some(whisper_log_callback), std::ptr::null_mut());
    }
}
