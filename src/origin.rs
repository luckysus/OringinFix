//! Origin 进程扫描与清理（与 UI 无关）。

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HWND, LPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
};

const TARGET: &str = "Origin64.exe";
/// 刚启动的 Origin 还没画出窗口，给它一点时间，避免把正在打开的实例误判成残留。
pub const MIN_AGE_SECS: u64 = 15;
/// 曾有过窗口的进程：窗口消失后再等一会，避开保存/退出动画中的误杀。
pub const CLOSE_GRACE_SECS: u64 = 12;
/// PPT 双击嵌入时，-Embedding 进程可能几十秒都还没画出窗口；自动清理要更保守。
pub const EMBEDDING_ORPHAN_AGE_SECS: u64 = 90;

/// `last_windowed`：上次见到可见窗口的时间（无记录 = 从未见过窗口）。
fn past_close_grace(pid: u32, last_windowed: &HashMap<u32, Instant>) -> bool {
    match last_windowed.get(&pid) {
        Some(t) => t.elapsed() >= Duration::from_secs(CLOSE_GRACE_SECS),
        None => true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcKind {
    Windowed,
    Automation,
    Stale,
    Protected,
}

#[derive(Debug, Clone)]
pub struct ProcInfo {
    pub pid: u32,
    pub kind: ProcKind,
    pub memory_bytes: u64,
    pub age_secs: Option<u64>,
    pub young: bool,
    /// 若由 Office OLE 拉起（如 PPT 双击嵌入图），标记来源便于识别。
    pub ole_source: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub procs: Vec<ProcInfo>,
}

impl Snapshot {
    pub fn is_idle(&self) -> bool {
        self.procs.is_empty()
    }

    pub fn by_kind(&self, kind: ProcKind) -> impl Iterator<Item = &ProcInfo> {
        self.procs.iter().filter(move |p| p.kind == kind)
    }

    /// 可清的残留（已过启动缓冲 + 关窗宽限）。
    pub fn stale_pids(&self, last_windowed: &HashMap<u32, Instant>) -> Vec<u32> {
        self.procs
            .iter()
            .filter(|p| {
                p.kind == ProcKind::Stale && !p.young && past_close_grace(p.pid, last_windowed)
            })
            .map(|p| p.pid)
            .collect()
    }

    pub fn auto_pids(&self) -> Vec<u32> {
        self.procs
            .iter()
            .filter(|p| p.kind == ProcKind::Automation)
            .map(|p| p.pid)
            .collect()
    }

    /// 可自动清理的无窗口自动化：
    /// - 曾经有过窗口（编辑后关掉）→ 须过关窗宽限
    /// - 从未见过窗口 → 必须足够老，避免 PPT 刚双击、窗口还没起来就被杀掉
    pub fn auto_pids_old(&self, last_windowed: &HashMap<u32, Instant>) -> Vec<u32> {
        self.procs
            .iter()
            .filter(|p| {
                if p.kind != ProcKind::Automation || p.young {
                    return false;
                }
                if last_windowed.contains_key(&p.pid) {
                    return past_close_grace(p.pid, last_windowed);
                }
                matches!(p.age_secs, Some(a) if a >= EMBEDDING_ORPHAN_AGE_SECS)
            })
            .map(|p| p.pid)
            .collect()
    }

    pub fn memory_of(&self, pids: &[u32]) -> u64 {
        self.procs
            .iter()
            .filter(|p| pids.contains(&p.pid))
            .map(|p| p.memory_bytes)
            .sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillError {
    AccessDenied,
    Other,
}

#[derive(Debug, Clone, Default)]
pub struct CleanReport {
    pub killed: usize,
    pub denied: usize,
    pub failed: usize,
    pub skipped_young: usize,
    pub freed_bytes: u64,
    pub killed_pids: Vec<u32>,
}

pub fn target_name() -> String {
    std::env::var("ORINGINFIX_TARGET").unwrap_or_else(|_| TARGET.to_owned())
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let b = bytes as f64;
    if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// `protect_args`：命令行包含其中任一项（忽略大小写）则归为受保护，不进自动清理。
pub fn snapshot(protect_args: &[String]) -> Snapshot {
    let pids = origin_pids();
    if pids.is_empty() {
        return Snapshot::default();
    }
    let windowed = pids_with_window(&pids);
    let parents = process_parent_map();
    let mut procs = Vec::with_capacity(pids.len());
    for pid in pids {
        let line = command_line(pid).unwrap_or_default();
        let mem = working_set(pid).unwrap_or(0);
        let age = age_secs(pid);
        // 拿不到年龄时宁可当「刚启动」，避免误杀正在拉起的 OLE 实例。
        let young = match age {
            Some(a) => a < MIN_AGE_SECS,
            None => true,
        };
        let embedding = is_embedding(&line);
        let ole_source = ole_label(pid, &parents);
        let kind = if windowed.contains(&pid) {
            ProcKind::Windowed
        } else if embedding {
            // PPT/Word 双击嵌入图、COM 脚本等：无窗口时归自动化。
            ProcKind::Automation
        } else if is_protected(&line, protect_args) {
            ProcKind::Protected
        } else {
            ProcKind::Stale
        };
        procs.push(ProcInfo {
            pid,
            kind,
            memory_bytes: mem,
            age_secs: age,
            young: young && matches!(kind, ProcKind::Stale | ProcKind::Automation),
            ole_source,
        });
    }
    Snapshot { procs }
}

pub fn report_check(protect_args: &[String]) -> String {
    let s = snapshot(protect_args);
    if s.is_idle() {
        return "check: idle".to_owned();
    }
    let stale: Vec<_> = s.by_kind(ProcKind::Stale).map(|p| p.pid).collect();
    let ages: Vec<String> = s
        .by_kind(ProcKind::Stale)
        .map(|p| match p.age_secs {
            Some(a) => format!("{a}s"),
            None => "?".to_owned(),
        })
        .collect();
    let windowed: Vec<_> = s.by_kind(ProcKind::Windowed).map(|p| p.pid).collect();
    let auto: Vec<_> = s.by_kind(ProcKind::Automation).map(|p| p.pid).collect();
    let protected: Vec<_> = s.by_kind(ProcKind::Protected).map(|p| p.pid).collect();
    format!(
        "check: stale=[{}] age=[{}] windowed=[{}] auto=[{}] protected=[{}]",
        join_pids(&stale),
        ages.join(","),
        join_pids(&windowed),
        join_pids(&auto),
        join_pids(&protected),
    )
}

pub fn release(protect_args: &[String], zh: bool) -> (String, CleanReport) {
    let empty = HashMap::new();
    release_with(protect_args, false, &empty, zh)
}

/// `include_embedding`：同时清理「可自动清理」的无窗口 -Embedding。
/// `last_windowed`：上次见到可见窗口的时间（关窗宽限 / 区分 PPT 未出窗）。
pub fn release_with(
    protect_args: &[String],
    include_embedding: bool,
    last_windowed: &HashMap<u32, Instant>,
    zh: bool,
) -> (String, CleanReport) {
    let s = snapshot(protect_args);
    let mut pids = s.stale_pids(last_windowed);
    if include_embedding {
        pids.extend(s.auto_pids_old(last_windowed));
    }
    pids.sort_unstable();
    pids.dedup();
    let mem = s.memory_of(&pids);
    let (msg, mut report) = clean_pids(pids, false, Some(&s), zh);
    report.freed_bytes = if report.killed > 0 { mem } else { 0 };
    let msg = with_freed(msg, &report, zh);
    (msg, report)
}

pub fn release_auto(protect_args: &[String], zh: bool) -> (String, CleanReport) {
    let s = snapshot(protect_args);
    let pids = s.auto_pids();
    let mem = s.memory_of(&pids);
    let (msg, mut report) = clean_pids(pids, true, Some(&s), zh);
    report.freed_bytes = if report.killed > 0 { mem } else { 0 };
    let msg = with_freed(msg, &report, zh);
    (msg, report)
}

pub fn release_selected(pids: &[u32], skip_young: bool, zh: bool) -> (String, CleanReport) {
    let s = snapshot(&[]);
    let mem = s.memory_of(pids);
    let filtered: Vec<u32> = if skip_young {
        pids.iter()
            .copied()
            .filter(|pid| {
                s.procs
                    .iter()
                    .find(|p| p.pid == *pid)
                    .is_none_or(|p| !p.young)
            })
            .collect()
    } else {
        pids.to_vec()
    };
    let young_skip = pids.len().saturating_sub(filtered.len());
    let (msg, mut report) = clean_pids(filtered, false, Some(&s), zh);
    report.skipped_young += young_skip;
    report.freed_bytes = if report.killed > 0 { mem } else { 0 };
    let msg = with_freed(msg, &report, zh);
    (msg, report)
}

fn with_freed(msg: String, report: &CleanReport, zh: bool) -> String {
    if report.killed > 0 && report.freed_bytes > 0 {
        let bytes = format_bytes(report.freed_bytes);
        if zh {
            format!("{msg}，约释放 {bytes}")
        } else {
            format!("{msg}, freed ~{bytes}")
        }
    } else {
        msg
    }
}

fn clean_pids(
    pids: Vec<u32>,
    automation: bool,
    prior: Option<&Snapshot>,
    zh: bool,
) -> (String, CleanReport) {
    let mut report = CleanReport::default();

    if pids.is_empty() {
        let msg = if automation {
            if zh {
                "没有可清理的自动化实例".to_owned()
            } else {
                "No automation instances to clean".to_owned()
            }
        } else if prior.is_some_and(|s| s.is_idle()) {
            if zh {
                "Origin 未在运行".to_owned()
            } else {
                "Origin is not running".to_owned()
            }
        } else if let Some(s) = prior {
            format_status(s, zh)
        } else if zh {
            "没有可清理的进程".to_owned()
        } else {
            "Nothing to clean".to_owned()
        };
        return (msg, report);
    }

    let young: Vec<u32> = if automation {
        Vec::new()
    } else {
        pids.iter()
            .copied()
            .filter(|pid| matches!(age_secs(*pid), Some(a) if a < MIN_AGE_SECS))
            .collect()
    };
    let old: Vec<u32> = pids
        .iter()
        .copied()
        .filter(|p| !young.contains(p))
        .collect();
    report.skipped_young = young.len();

    for pid in old {
        // 动手前再看一眼：若又出现可见窗口，绝不杀（用户可能刚重新打开/取消退出）。
        if !pids_with_window(&[pid]).is_empty() {
            report.skipped_young += 1;
            continue;
        }
        match terminate(pid) {
            Ok(()) => {
                // 必须以进程快照为准：TerminateProcess 后 OpenProcess 可能已失败，
                // 但 Toolhelp 里还能扫到，这时若当成已死再 refresh，列表又会刷回来。
                if wait_until_gone(pid, Duration::from_millis(1500)) {
                    report.killed += 1;
                    report.killed_pids.push(pid);
                } else {
                    // 再补一刀
                    let _ = terminate(pid);
                    if wait_until_gone(pid, Duration::from_millis(1500)) {
                        report.killed += 1;
                        report.killed_pids.push(pid);
                    } else {
                        report.failed += 1;
                    }
                }
            }
            Err(KillError::AccessDenied) => report.denied += 1,
            Err(KillError::Other) => report.failed += 1,
        }
    }

    (format_clean_msg(&report, automation, zh), report)
}

/// 是否仍出现在 Origin 进程快照中（比 OpenProcess/退出码更贴近列表展示）。
fn in_origin_snapshot(pid: u32) -> bool {
    origin_pids().contains(&pid)
}

fn wait_until_gone(pid: u32, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if !in_origin_snapshot(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !in_origin_snapshot(pid)
}

pub fn format_status(s: &Snapshot, zh: bool) -> String {
    if s.is_idle() {
        return if zh {
            "Origin 未在运行".to_owned()
        } else {
            "Origin is not running".to_owned()
        };
    }
    let stale = s.by_kind(ProcKind::Stale).count();
    let windowed = s.by_kind(ProcKind::Windowed).count();
    let auto = s.by_kind(ProcKind::Automation).count();
    let protected = s.by_kind(ProcKind::Protected).count();
    let mut parts = Vec::new();
    if zh {
        if stale > 0 {
            parts.push(format!("待清理 {stale}"));
        }
        if windowed > 0 {
            parts.push(format!("在用 {windowed}"));
        }
        if auto > 0 {
            parts.push(format!("自动化 {auto}"));
        }
        if protected > 0 {
            parts.push(format!("保护 {protected}"));
        }
        if parts.is_empty() {
            "Origin 正在使用中".to_owned()
        } else {
            parts.join(" / ")
        }
    } else {
        if stale > 0 {
            parts.push(format!("leftover {stale}"));
        }
        if windowed > 0 {
            parts.push(format!("in use {windowed}"));
        }
        if auto > 0 {
            parts.push(format!("automation {auto}"));
        }
        if protected > 0 {
            parts.push(format!("protected {protected}"));
        }
        if parts.is_empty() {
            "Origin is in use".to_owned()
        } else {
            parts.join(" / ")
        }
    }
}

fn format_clean_msg(r: &CleanReport, automation: bool, zh: bool) -> String {
    let mut parts = Vec::new();
    if zh {
        let kind = if automation { "自动化实例" } else { "进程" };
        if r.killed > 0 {
            parts.push(format!("已清理 {} 个{kind}", r.killed));
        }
        if r.denied > 0 {
            parts.push(format!("{} 个权限不足", r.denied));
        }
        if r.failed > 0 {
            parts.push(format!("{} 个失败", r.failed));
        }
        if r.skipped_young > 0 {
            parts.push(format!("跳过 {} 个刚启动的", r.skipped_young));
        }
        if parts.is_empty() {
            if r.skipped_young > 0 {
                "进程刚启动，暂未处理".to_owned()
            } else {
                format!("没有可清理的{kind}")
            }
        } else {
            parts.join("，")
        }
    } else {
        let kind = if automation {
            "automation instance(s)"
        } else {
            "process(es)"
        };
        if r.killed > 0 {
            parts.push(format!("Cleaned {} {kind}", r.killed));
        }
        if r.denied > 0 {
            parts.push(format!("{} access denied", r.denied));
        }
        if r.failed > 0 {
            parts.push(format!("{} failed", r.failed));
        }
        if r.skipped_young > 0 {
            parts.push(format!("skipped {} just-started", r.skipped_young));
        }
        if parts.is_empty() {
            if r.skipped_young > 0 {
                "Process just started; skipped".to_owned()
            } else {
                format!("No {kind} to clean")
            }
        } else {
            parts.join(", ")
        }
    }
}

fn join_pids(pids: &[u32]) -> String {
    pids.iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn is_embedding(line: &str) -> bool {
    line.split_whitespace()
        .any(|arg| arg.eq_ignore_ascii_case("-embedding"))
}

fn is_protected(line: &str, protect_args: &[String]) -> bool {
    if protect_args.is_empty() {
        return false;
    }
    let lower = line.to_ascii_lowercase();
    protect_args.iter().any(|a| {
        let a = a.trim();
        !a.is_empty() && lower.contains(&a.to_ascii_lowercase())
    })
}

fn working_set(pid: u32) -> Option<u64> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut counters = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..std::mem::zeroed()
        };
        let ok = GetProcessMemoryInfo(
            handle,
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        ok.then_some(counters.WorkingSetSize as u64)
    }
}

fn origin_pids() -> Vec<u32> {
    let target = target_name();
    process_rows()
        .into_iter()
        .filter(|(_, name, _)| name.eq_ignore_ascii_case(&target))
        .map(|(pid, _, _)| pid)
        .collect()
}

fn process_rows() -> Vec<(u32, String, u32)> {
    let mut rows = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return rows;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..std::mem::zeroed()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let name: String = entry
                    .szExeFile
                    .iter()
                    .take_while(|c| **c != 0)
                    .map(|c| char::from_u32(*c as u32).unwrap_or('?'))
                    .collect();
                rows.push((entry.th32ProcessID, name, entry.th32ParentProcessID));
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    rows
}

fn process_parent_map() -> std::collections::HashMap<u32, String> {
    let rows = process_rows();
    let by_pid: std::collections::HashMap<u32, String> = rows
        .iter()
        .map(|(pid, name, _)| (*pid, name.clone()))
        .collect();
    let mut parents = std::collections::HashMap::new();
    for (pid, _, ppid) in &rows {
        if let Some(name) = by_pid.get(ppid) {
            parents.insert(*pid, name.clone());
        }
    }
    parents
}

fn ole_label(pid: u32, parents: &std::collections::HashMap<u32, String>) -> Option<String> {
    let parent = parents.get(&pid)?;
    let lower = parent.to_ascii_lowercase();
    if lower.starts_with("powerpnt") {
        Some("PPT".into())
    } else if lower.starts_with("excel") {
        Some("Excel".into())
    } else if lower.starts_with("winword") {
        Some("Word".into())
    } else if lower.starts_with("outlook") {
        Some("Outlook".into())
    } else {
        None
    }
}

struct Scan {
    pids: Vec<u32>,
    active: Vec<u32>,
}

unsafe extern "system" fn on_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let scan = unsafe { &mut *(lparam.0 as *mut Scan) };
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if scan.pids.contains(&pid)
            && IsWindowVisible(hwnd).as_bool()
            && GetWindowTextLengthW(hwnd) > 0
            && !scan.active.contains(&pid)
        {
            scan.active.push(pid);
        }
    }
    BOOL(1)
}

fn pids_with_window(pids: &[u32]) -> Vec<u32> {
    let mut scan = Scan {
        pids: pids.to_vec(),
        active: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(on_window), LPARAM(&mut scan as *mut Scan as isize));
    }
    scan.active
}

fn command_line(pid: u32) -> Option<String> {
    use windows::Wdk::System::Threading::{
        NtQueryInformationProcess, ProcessCommandLineInformation,
    };
    use windows::Win32::Foundation::UNICODE_STRING;

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u8; 4096];
        let mut needed = 0u32;
        let mut status = NtQueryInformationProcess(
            handle,
            ProcessCommandLineInformation,
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
            &mut needed,
        );
        if status.0 < 0 && needed as usize > buf.len() {
            buf.resize(needed as usize, 0);
            status = NtQueryInformationProcess(
                handle,
                ProcessCommandLineInformation,
                buf.as_mut_ptr().cast(),
                buf.len() as u32,
                &mut needed,
            );
        }
        let line = (status.0 >= 0).then(|| {
            let us = buf.as_ptr().cast::<UNICODE_STRING>().read_unaligned();
            let len = (us.Length / 2) as usize;
            if len == 0 || us.Buffer.0.is_null() {
                String::new()
            } else {
                String::from_utf16_lossy(std::slice::from_raw_parts(us.Buffer.0, len))
            }
        });
        let _ = CloseHandle(handle);
        line
    }
}

fn age_secs(pid: u32) -> Option<u64> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user).is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return None;
        }
        let created = ((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()?
            .as_secs()
            + 11_644_473_600;
        Some(now.saturating_sub(created / 10_000_000))
    }
}

fn is_access_denied(err: &windows::core::Error) -> bool {
    let code = err.code().0 as u32;
    code == 5 || code == 0x8007_0005
}

pub fn terminate(pid: u32) -> Result<(), KillError> {
    unsafe {
        let handle = match OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            false,
            pid,
        ) {
            Ok(h) => h,
            Err(e) if is_access_denied(&e) => return Err(KillError::AccessDenied),
            Err(_) => return Err(KillError::Other),
        };

        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let same = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok()
            && std::path::Path::new(&String::from_utf16_lossy(&buf[..len as usize]))
                .file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&target_name()));

        let result = if !same {
            Err(KillError::Other)
        } else {
            match TerminateProcess(handle, 1) {
                Ok(()) => Ok(()),
                Err(e) if is_access_denied(&e) => Err(KillError::AccessDenied),
                Err(_) => Err(KillError::Other),
            }
        };
        let _ = CloseHandle(handle);
        result
    }
}
