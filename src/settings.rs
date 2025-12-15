//! Application settings persistence using dconf
//!
//! Settings are stored in dconf under `/com/adlib/voice-recorder/`

use log::error;

const DCONF_PATH: &str = "/com/adlib/voice-recorder/";

/// Keys for dconf settings
mod keys {
    pub const SELECTED_MODEL: &str = "selected-model";
    pub const USE_GPU: &str = "use-gpu";
    pub const CONFIRM_ON_DELETE: &str = "confirm-on-delete";
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
        error!("Failed to save selected model to dconf: {}", e);
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
        error!("Failed to save GPU setting to dconf: {}", e);
    }
}

/// Get the confirm on delete setting from dconf (defaults to true)
pub fn get_confirm_on_delete() -> bool {
    let key = format!("{}{}", DCONF_PATH, keys::CONFIRM_ON_DELETE);
    dconf_rs::get_boolean(&key).unwrap_or(true)
}

/// Set the confirm on delete setting in dconf
pub fn set_confirm_on_delete(confirm: bool) {
    let key = format!("{}{}", DCONF_PATH, keys::CONFIRM_ON_DELETE);
    if let Err(e) = dconf_rs::set_boolean(&key, confirm) {
        error!("Failed to save confirm on delete setting to dconf: {}", e);
    }
}
