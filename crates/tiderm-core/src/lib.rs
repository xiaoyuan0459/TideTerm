//! TideTerm core library — de-shelled pure-Rust logic (B-series).
//!
//! Holds the transport- / Tauri- / Slint-free parts of the backend: session config
//! persistence, known-hosts verification, the master-password vault, crash-recovery
//! snapshots, OneKey accounts and sync-input state. The Qt shell (`native/`)
//! depends on this crate and calls these functions directly through its `cxx`
//! bridge; the `#[tauri::command]` wrappers that used to sit in `src-tauri` are gone.

pub mod ai;
pub mod app_log;
pub mod auth;
pub mod completion;
pub mod config;
pub mod crash_recovery;
pub mod error;
pub mod event;
pub mod known_hosts;
pub mod master_password;
pub mod onekey;
// B-series: protocol layer (PTY / SSH / SFTP transports) + session manager,
// migrated so the native shell drives the same async transports as the app.
pub mod modem;
pub mod path_util;
pub mod protocol;
pub mod read_loop;
pub mod session;
pub mod session_options;
pub mod sftp;
mod sftp_listing;
pub mod sftp_policy;
#[cfg(test)]
mod sftp_policy_tests;
pub mod sftp_resume;
mod sftp_stream;
pub mod sftp_types;
pub mod sync_input;
pub mod system_monitor;
pub mod terminal_config;
pub mod throttle;

/// App config directory (`{config_dir}/com.tideterm.app`), created on demand.
pub fn config_dir() -> Result<std::path::PathBuf, std::string::String> {
    base_config_dir()
}

/// Choose an explicit profile before configuration is loaded (CLI --config-dir
/// and isolated diagnostics). No fallback to the ordinary user profile on error.
pub fn set_config_directory(path: std::path::PathBuf) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("config directory must be absolute".into());
    }
    std::fs::create_dir_all(&path).map_err(|e| format!("create config dir: {e}"))?;
    PROFILE_DIRECTORY
        .set(path)
        .map_err(|_| "config directory already set".into())
}

static PROFILE_DIRECTORY: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// The base config directory before ensuring it exists.
fn base_config_dir() -> Result<std::path::PathBuf, String> {
    if let Some(path) = PROFILE_DIRECTORY.get() {
        return Ok(path.clone());
    }
    #[cfg(test)]
    {
        if let Some(path) = TEST_DIR_OVERRIDE.lock().unwrap().clone() {
            std::fs::create_dir_all(&path).map_err(|e| format!("create config dir: {e}"))?;
            return Ok(path);
        }
    }
    let base = dirs::config_dir()
        .ok_or_else(|| "cannot determine config directory".to_string())?
        .join("com.tideterm.app");
    std::fs::create_dir_all(&base).map_err(|e| format!("create config dir: {e}"))?;
    Ok(base)
}

#[cfg(test)]
static TEST_DIR_OVERRIDE: std::sync::Mutex<Option<std::path::PathBuf>> =
    std::sync::Mutex::new(None);

/// Test-only: point the config directory at a temp dir for hermetic tests.
#[cfg(test)]
pub fn set_test_config_dir(path: std::path::PathBuf) {
    *TEST_DIR_OVERRIDE.lock().unwrap() = Some(path);
}

/// A global test lock so tests that mutate [`TEST_DIR_OVERRIDE`] run serially.
#[cfg(test)]
pub fn test_config_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap()
}
