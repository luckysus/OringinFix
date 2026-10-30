//! `%AppData%/OringinFix/config.toml`

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    #[default]
    Tray,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    Zh,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub auto_clean: bool,
    pub interval_secs: u64,
    pub start_with_windows: bool,
    pub close_behavior: CloseBehavior,
    pub silent_start: bool,
    pub notify_on_clean: bool,
    pub auto_clean_embedding: bool,
    pub protect_args: Vec<String>,
    pub language: Language,
    pub theme: ThemeMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auto_clean: true,
            interval_secs: 5,
            start_with_windows: false,
            close_behavior: CloseBehavior::Tray,
            silent_start: true,
            notify_on_clean: true,
            auto_clean_embedding: true,
            protect_args: Vec::new(),
            language: Language::Zh,
            theme: ThemeMode::System,
        }
    }
}

impl Config {
    pub fn dir() -> PathBuf {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("OringinFix")
    }

    pub fn path() -> PathBuf {
        Self::dir().join("config.toml")
    }

    pub fn load() -> Self {
        let path = Self::path();
        let Ok(text) = fs::read_to_string(&path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self) {
        let dir = Self::dir();
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        if let Ok(text) = toml::to_string_pretty(self) {
            let _ = fs::write(Self::path(), text);
        }
    }

    pub fn protect_text(&self) -> String {
        self.protect_args.join(", ")
    }

    pub fn set_protect_text(&mut self, text: &str) {
        self.protect_args = text
            .split([',', '\n', ';'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
}

/// Windows：AppsUseLightTheme == 0 表示深色应用主题。
pub fn system_dark() -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
    };
    unsafe {
        let mut key = std::mem::zeroed();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            None,
            KEY_READ,
            &mut key,
        )
        .is_err()
        {
            return false;
        }
        let mut value = 1u32;
        let mut size = 4u32;
        let mut ty = REG_DWORD;
        let ok = RegQueryValueExW(
            key,
            w!("AppsUseLightTheme"),
            None,
            Some(&mut ty),
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
        .is_ok();
        let _ = RegCloseKey(key);
        ok && value == 0
    }
}

pub fn resolve_dark(mode: ThemeMode) -> bool {
    match mode {
        ThemeMode::Light => false,
        ThemeMode::Dark => true,
        ThemeMode::System => system_dark(),
    }
}
