//! Local machine settings: where Terminal A lives and who this terminal is.

use gloo_storage::{LocalStorage, Storage};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const CONFIG_KEY: &str = "hiffey.pos.config";
const TOKEN_KEY: &str = "hiffey.pos.token";

pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:47800";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub base_url: String,
    pub terminal_id: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            terminal_id: Uuid::now_v7().to_string(),
        }
    }
}

impl Config {
    /// Reads this PC's settings; the terminal id is generated once and kept.
    #[must_use]
    pub fn load() -> Self {
        let config: Config = LocalStorage::get(CONFIG_KEY).unwrap_or_default();
        config.save();
        config
    }

    pub fn save(&self) {
        let _ = LocalStorage::set(CONFIG_KEY, self);
    }

    pub fn with_connection(base_url: &str, terminal_id: &str) -> Self {
        Self {
            base_url: normalize_base_url(base_url),
            terminal_id: if terminal_id.trim().is_empty() {
                Uuid::now_v7().to_string()
            } else {
                terminal_id.trim().to_owned()
            },
        }
    }
}

/// Accepts `192.168.1.10:47800` or a full URL and returns a clean base URL.
#[must_use]
pub fn normalize_base_url(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return DEFAULT_BASE_URL.to_owned();
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_owned()
    } else {
        format!("http://{trimmed}")
    }
}

#[must_use]
pub fn load_token() -> Option<String> {
    LocalStorage::get(TOKEN_KEY).ok()
}

pub fn save_token(token: &str) {
    let _ = LocalStorage::set(TOKEN_KEY, token);
}

pub fn clear_token() {
    LocalStorage::delete(TOKEN_KEY);
}
