//! 最近清理记录：`%AppData%/OringinFix/clean_log.json`

use std::fs;

use serde::{Deserialize, Serialize};

use crate::config::Config;

const MAX_ENTRIES: usize = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub time: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LogFile {
    entries: Vec<LogEntry>,
}

fn path() -> std::path::PathBuf {
    Config::dir().join("clean_log.json")
}

pub fn load() -> Vec<LogEntry> {
    let Ok(text) = fs::read_to_string(path()) else {
        return Vec::new();
    };
    serde_json::from_str::<LogFile>(&text)
        .map(|f| f.entries)
        .unwrap_or_default()
}

pub fn append(message: impl Into<String>) {
    let mut entries = load();
    let time = chrono_lite_now();
    entries.insert(
        0,
        LogEntry {
            time,
            message: message.into(),
        },
    );
    entries.truncate(MAX_ENTRIES);
    let _ = fs::create_dir_all(Config::dir());
    if let Ok(text) = serde_json::to_string_pretty(&LogFile { entries }) {
        let _ = fs::write(path(), text);
    }
}

fn chrono_lite_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let Ok(dur) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return "?".to_owned();
    };
    // 本地时间用 Windows API 更准；这里用简单 UTC+8 偏移够用。
    let secs = dur.as_secs() as i64 + 8 * 3600;
    let days = secs / 86400;
    let tod = secs.rem_euclid(86400) as u32;
    let h = tod / 3600;
    let m = (tod % 3600) / 60;
    let s = tod % 60;
    // 1970-01-01 起算的粗略月日（足够日志可读）
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}")
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    // Howard Hinnant civil_from_days
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64 + era * 400) as i32;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
