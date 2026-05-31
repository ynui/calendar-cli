use std::path::PathBuf;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::app::ThemeKind;

include!(concat!(env!("OUT_DIR"), "/credentials.rs"));

#[derive(Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub first_day_of_week: u8,
    #[serde(default)]
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            first_day_of_week: 0,
            theme: "default".to_string(),
        }
    }
}

impl Settings {
    pub fn theme_kind(&self) -> ThemeKind {
        self.theme.parse().unwrap_or(ThemeKind::Default)
    }
}

pub struct Config {
    pub credentials_path: PathBuf,
    pub token_path: PathBuf,
    pub settings_path: PathBuf,
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .context("config directory not found")?
            .join("calendar-cli");
        std::fs::create_dir_all(&config_dir)?;

        let credentials_path = config_dir.join("credentials.json");
        if !credentials_path.exists() {
            if let Some(creds) = EMBEDDED_CREDENTIALS {
                std::fs::write(&credentials_path, creds)?;
            }
        }

        Ok(Config {
            credentials_path,
            token_path: config_dir.join("token.json"),
            settings_path: config_dir.join("settings.json"),
        })
    }

    pub fn events_path(&self) -> PathBuf {
        self.credentials_path
            .parent()
            .expect("credentials_path should have a parent directory")
            .join("events.json")
    }

    pub fn load_settings(&self) -> Settings {
        std::fs::read_to_string(&self.settings_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

}
