//! 并行查询 GitHub / Gitee Releases；发现新版时可下载安装包并静默覆盖安装。

use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde::Deserialize;

const GITHUB_API: &str = "https://api.github.com/repos/luckysus/OringinFix/releases/latest";
const GITEE_API: &str = "https://gitee.com/api/v5/repos/neusu/OringinFix/releases/latest";
const GITHUB_RELEASES: &str = "https://github.com/luckysus/OringinFix/releases";
const GITEE_RELEASES: &str = "https://gitee.com/neusu/OringinFix/releases";
const SETUP_SUFFIX: &str = "-windows-x64.exe";
const SETUP_PREFIX: &str = "OringinFix-Setup-";

#[derive(Debug, Clone)]
pub enum CheckResult {
    UpToDate { latest: String },
    Available {
        latest: String,
        /// Release 页面
        url: String,
        /// 安装包直链；缺失时只能打开网页
        download_url: Option<String>,
    },
    /// 仓库能访问，但还没有任何 Release
    NoRelease,
    Failed(String),
}

/// 下载过程消息：进度 → 完成/失败（对齐 DBX 分阶段）。
#[derive(Debug, Clone)]
pub enum DownloadMsg {
    /// 可见百分比；`None` 表示未知总长
    Progress { percent: Option<u8> },
    Ready(PathBuf),
    Failed(String),
}

#[derive(Clone)]
struct RemoteRelease {
    version: String,
    url: String,
    download_url: Option<String>,
    source: &'static str,
}

#[derive(Deserialize)]
struct ReleaseJson {
    tag_name: String,
    html_url: Option<String>,
    #[serde(default)]
    assets: Vec<AssetJson>,
}

#[derive(Deserialize)]
struct AssetJson {
    name: String,
    browser_download_url: Option<String>,
}

pub fn check_latest(current: &str) -> CheckResult {
    let local = normalize_version(current);
    let (remotes, saw_no_release, errors) = fetch_both();

    if remotes.is_empty() {
        if saw_no_release {
            return CheckResult::NoRelease;
        }
        if errors.is_empty() {
            return CheckResult::Failed("GitHub / Gitee 均不可达".into());
        }
        return CheckResult::Failed(errors.join("；"));
    }

    // 取版本号最高的一条；同版本优先带安装包直链的，再优先 GitHub
    let best = remotes
        .into_iter()
        .max_by(|a, b| {
            cmp_version(&a.version, &b.version)
                .then_with(|| a.download_url.is_some().cmp(&b.download_url.is_some()))
                .then_with(|| (a.source == "GitHub").cmp(&(b.source == "GitHub")))
        })
        .expect("non-empty");

    match cmp_version(&local, &best.version) {
        std::cmp::Ordering::Less => CheckResult::Available {
            latest: best.version,
            url: best.url,
            download_url: best.download_url,
        },
        _ => CheckResult::UpToDate {
            latest: best.version,
        },
    }
}

/// 下载安装包到临时目录，经 `tx` 推送进度与结果。
pub fn download_installer(download_url: &str, version: &str, tx: &mpsc::Sender<DownloadMsg>) {
    let name = format!("{SETUP_PREFIX}{version}{SETUP_SUFFIX}");
    let path = std::env::temp_dir().join(&name);

    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(180)))
        .build()
        .new_agent();

    let resp = match agent
        .get(download_url)
        .header("User-Agent", "OringinFix-update")
        .call()
    {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(DownloadMsg::Failed(format!("下载失败：{e}")));
            return;
        }
    };

    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let _ = tx.send(DownloadMsg::Failed(format!("下载失败：HTTP {status}")));
        return;
    }

    let total = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&n| n > 0);

    let mut body = resp.into_body().into_reader();
    let mut file = match File::create(&path) {
        Ok(f) => f,
        Err(e) => {
            let _ = tx.send(DownloadMsg::Failed(format!("无法写入临时文件：{e}")));
            return;
        }
    };

    let _ = tx.send(DownloadMsg::Progress {
        percent: total.map(|_| 0),
    });

    let mut buf = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let mut last_pct: Option<u8> = None;
    loop {
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                let _ = tx.send(DownloadMsg::Failed(format!("写入失败：{e}")));
                return;
            }
        };
        if let Err(e) = file.write_all(&buf[..n]) {
            let _ = std::fs::remove_file(&path);
            let _ = tx.send(DownloadMsg::Failed(format!("写入失败：{e}")));
            return;
        }
        downloaded = downloaded.saturating_add(n as u64);
        // 仅在可见百分比变化时推送（DBX progress gate 的简化版）
        let pct = total.map(|t| {
            let p = downloaded.saturating_mul(100) / t;
            p.min(100) as u8
        });
        if pct != last_pct {
            last_pct = pct;
            let _ = tx.send(DownloadMsg::Progress { percent: pct });
        }
    }

    if let Err(e) = file.flush() {
        let _ = std::fs::remove_file(&path);
        let _ = tx.send(DownloadMsg::Failed(format!("写入失败：{e}")));
        return;
    }

    if let Some(t) = total {
        if downloaded < t {
            let _ = std::fs::remove_file(&path);
            let _ = tx.send(DownloadMsg::Failed(format!(
                "下载不完整（{downloaded}/{t}）"
            )));
            return;
        }
    }

    let _ = tx.send(DownloadMsg::Ready(path));
}

/// 静默覆盖安装（`/S /AUTORUN=1`）；装完由独立 cmd 删临时包。
pub fn launch_silent_installer(path: &std::path::Path) -> Result<(), String> {
    let path_str = path.to_string_lossy().replace('"', "");
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    // start /wait：等安装结束再 del；参数跟在 exe 后交给 NSIS
    let cmdline = format!(
        "start \"\" /wait \"{path_str}\" /S /AUTORUN=1 & del /f /q \"{path_str}\" >nul 2>&1"
    );
    std::process::Command::new("cmd")
        .args(["/C", &cmdline])
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("无法启动安装程序：{e}"))
}

/// 清掉 %TEMP% 里遗留的安装包（上次异常退出 / 删失败时的兜底）。
pub fn sweep_temp_installers() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(s) = name.to_str() else {
            continue;
        };
        if s.starts_with(SETUP_PREFIX) && s.ends_with(SETUP_SUFFIX) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

enum FetchOutcome {
    Ok(RemoteRelease),
    NoRelease,
    Err(String),
}

fn fetch_both() -> (Vec<RemoteRelease>, bool, Vec<String>) {
    let (tx, rx) = mpsc::channel();

    let tx_gh = tx.clone();
    thread::spawn(move || {
        let _ = tx_gh.send(fetch_one(
            GITHUB_API,
            GITHUB_RELEASES,
            "GitHub",
            "application/vnd.github+json",
        ));
    });

    thread::spawn(move || {
        let _ = tx.send(fetch_one(
            GITEE_API,
            GITEE_RELEASES,
            "Gitee",
            "application/json",
        ));
    });

    let mut out = Vec::new();
    let mut saw_no_release = false;
    let mut errors = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(14);
    for _ in 0..2 {
        let remain = deadline.saturating_duration_since(std::time::Instant::now());
        if remain.is_zero() {
            break;
        }
        match rx.recv_timeout(remain) {
            Ok(FetchOutcome::Ok(rel)) => out.push(rel),
            Ok(FetchOutcome::NoRelease) => saw_no_release = true,
            Ok(FetchOutcome::Err(e)) => errors.push(e),
            Err(_) => break,
        }
    }
    (out, saw_no_release, errors)
}

fn fetch_one(
    api: &str,
    fallback_url: &str,
    source: &'static str,
    accept: &str,
) -> FetchOutcome {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(12)))
        .build()
        .new_agent();

    let resp = match agent
        .get(api)
        .header("User-Agent", "OringinFix-update-check")
        .header("Accept", accept)
        .call()
    {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => return FetchOutcome::NoRelease,
        Err(e) => return FetchOutcome::Err(format!("{source}: {e}")),
    };

    let status = resp.status().as_u16();
    if status == 404 {
        return FetchOutcome::NoRelease;
    }
    if !(200..300).contains(&status) {
        return FetchOutcome::Err(format!("{source}: HTTP {status}"));
    }

    let body = match resp.into_body().read_to_string() {
        Ok(b) => b,
        Err(e) => return FetchOutcome::Err(format!("{source} read: {e}")),
    };

    let rel: ReleaseJson = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(_) => return FetchOutcome::Err(format!("{source}: bad json")),
    };

    let version = normalize_version(&rel.tag_name);
    let download_url = pick_setup_asset(&rel.assets);

    FetchOutcome::Ok(RemoteRelease {
        version,
        url: rel.html_url.unwrap_or_else(|| fallback_url.to_owned()),
        download_url,
        source,
    })
}

fn pick_setup_asset(assets: &[AssetJson]) -> Option<String> {
    assets
        .iter()
        .find(|a| {
            a.name.starts_with(SETUP_PREFIX)
                && a.name.ends_with(SETUP_SUFFIX)
                && a.name.to_ascii_lowercase().ends_with(".exe")
        })
        .and_then(|a| a.browser_download_url.clone())
        .or_else(|| {
            // 兼容命名略有差异：只要是 Setup + windows-x64.exe
            assets.iter().find_map(|a| {
                let n = a.name.to_ascii_lowercase();
                if n.contains("setup") && n.ends_with("windows-x64.exe") {
                    a.browser_download_url.clone()
                } else {
                    None
                }
            })
        })
}

fn normalize_version(s: &str) -> String {
    s.trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .split(|c: char| c == '-' || c == '+' || c.is_whitespace())
        .next()
        .unwrap_or("0")
        .to_string()
}

fn cmp_version(a: &str, b: &str) -> std::cmp::Ordering {
    let pa = parse_parts(a);
    let pb = parse_parts(b);
    let n = pa.len().max(pb.len());
    for i in 0..n {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

fn parse_parts(s: &str) -> Vec<u64> {
    s.split('.')
        .filter_map(|p| {
            let digits: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse().ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_order() {
        assert_eq!(cmp_version("0.1.0", "0.1.0"), std::cmp::Ordering::Equal);
        assert_eq!(cmp_version("0.1.0", "0.2.0"), std::cmp::Ordering::Less);
        assert_eq!(cmp_version("1.0.0", "0.9.9"), std::cmp::Ordering::Greater);
        assert_eq!(normalize_version("v1.2.3-beta"), "1.2.3");
    }

    #[test]
    fn pick_setup() {
        let assets = vec![
            AssetJson {
                name: "notes.txt".into(),
                browser_download_url: Some("https://x/notes".into()),
            },
            AssetJson {
                name: "OringinFix-Setup-0.1.1-windows-x64.exe".into(),
                browser_download_url: Some("https://x/setup.exe".into()),
            },
        ];
        assert_eq!(
            pick_setup_asset(&assets).as_deref(),
            Some("https://x/setup.exe")
        );
    }
}
