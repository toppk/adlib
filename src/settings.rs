//! Application settings persistence using dconf
//!
//! Settings are stored in dconf under `/com/adlib/voice-recorder/`

const DCONF_PATH: &str = "/com/adlib/voice-recorder/";

/// Keys for dconf settings
mod keys {
    pub const SELECTED_MODEL: &str = "selected-model";
    pub const USE_GPU: &str = "use-gpu";
}

/// Get the selected Whisper model name from dconf
pub fn get_selected_model() -> Option<String> {
    let key = format!("{}{}", DCONF_PATH, keys::SELECTED_MODEL);
    dconf_rs::get_string(&key).ok()
}

/// Set the selected Whisper model name in dconf
pub fn set_selected_model(model_name: &str) {
    let key = format!("{}{}", DCONF_PATH, keys::SELECTED_MODEL);
    if let Err(e) = dconf_rs::set_string(&key, model_name) {
        eprintln!("Failed to save selected model to dconf: {}", e);
    }
}

/// Get the GPU acceleration setting from dconf
pub fn get_use_gpu() -> bool {
    let key = format!("{}{}", DCONF_PATH, keys::USE_GPU);
    dconf_rs::get_boolean(&key).unwrap_or(false)
}

/// Set the GPU acceleration setting in dconf
pub fn set_use_gpu(use_gpu: bool) {
    let key = format!("{}{}", DCONF_PATH, keys::USE_GPU);
    if let Err(e) = dconf_rs::set_boolean(&key, use_gpu) {
        eprintln!("Failed to save GPU setting to dconf: {}", e);
    }
}
