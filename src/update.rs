//! 并行查询 GitHub / Gitee Releases，任一可达即可；两边都通时取较新版本。

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde::Deserialize;

const GITHUB_API: &str = "https://api.github.com/repos/luckysus/OringinFix/releases/latest";
const GITEE_API: &str = "https://gitee.com/api/v5/repos/neusu/OringinFix/releases/latest";
const GITHUB_RELEASES: &str = "https://github.com/luckysus/OringinFix/releases";
const GITEE_RELEASES: &str = "https://gitee.com/neusu/OringinFix/releases";

#[derive(Debug, Clone)]
pub enum CheckResult {
    UpToDate { latest: String },
    Available { latest: String, url: String },
    /// 仓库能访问，但还没有任何 Release
    NoRelease,
    Failed(String),
}

#[derive(Clone)]
struct RemoteRelease {
    version: String,
    url: String,
    source: &'static str,
}

#[derive(Deserialize)]
struct ReleaseJson {
    tag_name: String,
    html_url: Option<String>,
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

    // 取版本号最高的一条；同版本优先 GitHub 链接
    let best = remotes
        .into_iter()
        .max_by(|a, b| {
            cmp_version(&a.version, &b.version).then_with(|| {
                (b.source == "GitHub").cmp(&(a.source == "GitHub"))
            })
        })
        .expect("non-empty");

    match cmp_version(&local, &best.version) {
        std::cmp::Ordering::Less => CheckResult::Available {
            latest: best.version,
            url: best.url,
        },
        _ => CheckResult::UpToDate {
            latest: best.version,
        },
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

    FetchOutcome::Ok(RemoteRelease {
        version: normalize_version(&rel.tag_name),
        url: rel.html_url.unwrap_or_else(|| fallback_url.to_owned()),
        source,
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
}
