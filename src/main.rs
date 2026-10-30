#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use eframe::egui;
use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HWND, LPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
};

const TARGET: &str = "Origin64.exe";
// 刚启动的 Origin 还没画出窗口，给它一点时间，避免把正在打开的实例误判成残留。
const MIN_AGE_SECS: u64 = 10;

const ACCENT: egui::Color32 = egui::Color32::from_rgb(29, 29, 31);
const ACCENT_HOVER: egui::Color32 = egui::Color32::from_rgb(58, 58, 61);
const ACCENT_ACTIVE: egui::Color32 = egui::Color32::from_rgb(0, 0, 0);

const TEXT: egui::Color32 = egui::Color32::from_rgb(29, 29, 31);
const MUTED: egui::Color32 = egui::Color32::from_rgb(110, 110, 115);

enum Decision {
    Idle,
    InUse { windowed: usize, auto: usize },
    Stale {
        stale: Vec<u32>,
        windowed: usize,
        auto: usize,
    },
}

// 按进程判断：自己有可见窗口的实例在用，COM 自动化实例（-Embedding）也算在用，其余才清理。
fn decide() -> Decision {
    let pids = origin_pids();
    if pids.is_empty() {
        return Decision::Idle;
    }
    let windowed = pids_with_window(&pids);
    let auto: Vec<u32> = pids
        .iter()
        .copied()
        .filter(|p| !windowed.contains(p) && is_automation(*p))
        .collect();
    let stale: Vec<u32> = pids
        .iter()
        .copied()
        .filter(|p| !windowed.contains(p) && !auto.contains(p))
        .collect();
    if stale.is_empty() {
        Decision::InUse {
            windowed: windowed.len(),
            auto: auto.len(),
        }
    } else {
        Decision::Stale {
            stale,
            windowed: windowed.len(),
            auto: auto.len(),
        }
    }
}

fn target_name() -> String {
    // 测试用：ORINGINFIX_TARGET 可把监控目标换成别的进程名。
    std::env::var("ORINGINFIX_TARGET").unwrap_or_else(|_| TARGET.to_owned())
}

fn origin_pids() -> Vec<u32> {
    let target = target_name();
    let mut pids = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return pids;
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
                if name.eq_ignore_ascii_case(&target) {
                    pids.push(entry.th32ProcessID);
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    pids
}

// 真正的 Origin 窗口是"可见且标题非空"；最小化的窗口仍算可见，关窗后残留的进程只剩
// IME、OLE 之类空标题的内部窗口。记下每个 pid 是否有窗口，才能只清残留、不碰在用的实例。
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

// Win32 没有公开 API 读别的进程命令行，只能走 ntdll；拿不到（无权限/已退出）就当空。
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

// COM 自动化（Origin.Application）拉起的实例命令行带 -Embedding，没有窗口也在正经干活。
fn is_automation(pid: u32) -> bool {
    command_line(pid).is_some_and(|line| {
        line.split_whitespace()
            .any(|arg| arg.eq_ignore_ascii_case("-embedding"))
    })
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
            + 11_644_473_600; // FILETIME 纪元到 UNIX 纪元相差的秒数
        Some(now.saturating_sub(created / 10_000_000))
    }
}

fn terminate(pid: u32) -> bool {
    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE, false, pid) {
            Ok(handle) => {
                // 快照和杀进程之间 PID 可能已被系统复用，在同一句柄上复核镜像文件名再动手。
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
                let ok = same && TerminateProcess(handle, 1).is_ok();
                let _ = CloseHandle(handle);
                ok
            }
            Err(_) => false,
        }
    }
}

// 返回（给用户看的话，真正结束掉的进程数）
fn release() -> (String, usize) {
    match decide() {
        Decision::Idle => ("Origin 未在运行".to_owned(), 0),
        Decision::InUse { windowed, auto } => (in_use_text(windowed, auto), 0),
        Decision::Stale {
            stale,
            windowed,
            auto,
        } => {
            let young: Vec<u32> = stale
                .iter()
                .copied()
                .filter(|pid| matches!(age_secs(*pid), Some(a) if a < MIN_AGE_SECS))
                .collect();
            let old: Vec<u32> = stale.iter().copied().filter(|p| !young.contains(p)).collect();
            let killed = old.iter().filter(|pid| terminate(**pid)).count();
            let msg = match (killed, young.len()) {
                (0, _) => "进程刚启动，暂未处理".to_owned(),
                (k, 0) if windowed > 0 => format!("已清理 {k} 个残留进程，{windowed} 个在用"),
                (k, 0) => format!("已清理 {k} 个残留进程"),
                (k, y) => format!("已清理 {k} 个，跳过 {y} 个刚启动的"),
            };
            let msg = if auto > 0 {
                format!("{msg}，{auto} 个自动化实例未动")
            } else {
                msg
            };
            (msg, killed)
        }
    }
}

// 只报告当前状态，不动任何进程。
fn peek() -> String {
    match decide() {
        Decision::Idle => "Origin 未在运行".to_owned(),
        Decision::InUse { windowed, auto } => in_use_text(windowed, auto),
        Decision::Stale {
            stale,
            windowed,
            auto,
        } => {
            let mut text = format!("{} 个残留进程待清理", stale.len());
            if windowed > 0 {
                text.push_str(&format!("，{windowed} 个在用"));
            }
            if auto > 0 {
                text.push_str(&format!("，{auto} 个自动化实例"));
            }
            text
        }
    }
}

// 自动化实例分不清"还在算"和"崩溃残留"，只能提醒它们存在，清不清由用户判断。
fn in_use_text(windowed: usize, auto: usize) -> String {
    match (windowed, auto) {
        (_, 0) => "Origin 正在使用中".to_owned(),
        (0, m) => format!("{m} 个自动化实例在后台（不会被清理）"),
        (_, m) => format!("Origin 正在使用中，{m} 个自动化实例"),
    }
}

fn report() -> String {
    match decide() {
        Decision::Idle => "check: idle".to_owned(),
        Decision::InUse { windowed, auto } => {
            format!("check: in use, windowed={windowed} auto={auto}")
        }
        Decision::Stale {
            stale,
            windowed,
            auto,
        } => {
            let ids: Vec<String> = stale.iter().map(|p| p.to_string()).collect();
            let ages: Vec<String> = stale
                .iter()
                .map(|p| match age_secs(*p) {
                    Some(a) => format!("{a}s"),
                    None => "?".to_owned(),
                })
                .collect();
            format!(
                "check: stale pids=[{}] age=[{}] windowed={windowed} auto={auto}",
                ids.join(","),
                ages.join(",")
            )
        }
    }
}

const BOLD: &str = "bold";

fn bold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(BOLD.into()))
}

const INTER: &[u8] = include_bytes!("../assets/Inter-Regular.ttf");
// 系统里的 NotoSansSC-VF 默认轴是 wght=100（极细），直接用会渲染成发丝，
// 所以这里内嵌一个用 fonttools 从该可变字体抽出的 wght=400 静态实例。
const CJK: &[u8] = include_bytes!("../assets/NotoSansSC-Regular.ttf");

fn configure(ctx: &egui::Context) {
    // 自己铺底色；面板透明，才让系统 Acrylic/Mica 透出来。
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = egui::Color32::TRANSPARENT;
    visuals.window_fill = egui::Color32::TRANSPARENT;
    ctx.set_visuals(visuals);
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "inter".to_owned(),
        Arc::new(egui::FontData::from_static(INTER)),
    );
    fonts.font_data.insert(
        "cjk".to_owned(),
        Arc::new(egui::FontData::from_static(CJK)),
    );

    // 拉丁走 Inter，中文落到思源黑体，最后才用 egui 自带的符号字体兜底。
    let mut chain = vec!["inter".to_owned(), "cjk".to_owned()];
    chain.extend(
        fonts
            .families
            .get(&egui::FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    fonts.families.insert(egui::FontFamily::Proportional, chain.clone());
    fonts.families.insert(egui::FontFamily::Name(BOLD.into()), chain);
    ctx.set_fonts(fonts);
}

struct App {
    status: Arc<Mutex<String>>,
    glass: bool,
    auto: Arc<AtomicBool>,
}

// 标题栏按钮：悬停时浮出一层淡背景，光标变手型，点起来有反馈。
fn title_button(ui: &mut egui::Ui, rect: egui::Rect, glyph: &str, id: egui::Id) -> bool {
    let resp = ui
        .interact(rect, id, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let bg = if resp.is_pointer_button_down_on() {
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 40)
    } else if resp.hovered() {
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 18)
    } else {
        egui::Color32::TRANSPARENT
    };
    if bg != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(7), bg);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(16.0),
        MUTED,
    );
    resp.clicked()
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let rect = ui.max_rect();
        // 圆角交给 DWM（窗口已被设为圆角），自己铺满矩形，避免在圆角处露出亚克力底色。
        // glass 时遮罩要够透，否则 DWM Acrylic/Mica 几乎看不见。
        let fill = if self.glass {
            egui::Color32::from_rgba_unmultiplied(252, 252, 254, 148)
        } else {
            egui::Color32::from_rgb(248, 250, 252)
        };
        ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, fill);

        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(30.0, 26.0))));

        // 无边框窗口：标题栏自己画，同时负责拖动和关闭。
        let (header, drag) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        ui.painter().text(
            header.left_center(),
            egui::Align2::LEFT_CENTER,
            "Origin 后台进程清理",
            bold(18.0),
            TEXT,
        );
        let btn = 28.0;
        let close_rect = egui::Rect::from_center_size(
            header.right_center() - egui::vec2(btn / 2.0, 0.0),
            egui::vec2(btn, btn),
        );
        let close_id = ui.scope_id().with("close");
        if title_button(&mut ui, close_rect, "×", close_id) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let min_rect = egui::Rect::from_center_size(
            header.right_center() - egui::vec2(btn / 2.0 + btn + 8.0, 0.0),
            egui::vec2(btn, btn),
        );
        let min_id = ui.scope_id().with("min");
        if title_button(&mut ui, min_rect, "−", min_id) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }

        ui.add_space(18.0);
        ui.scope(|ui| {
            let widgets = &mut ui.style_mut().visuals.widgets;
            for v in [&mut widgets.inactive, &mut widgets.hovered, &mut widgets.active] {
                v.weak_bg_fill = ACCENT;
                v.bg_stroke = egui::Stroke::NONE;
                v.corner_radius = egui::CornerRadius::same(10);
            }
            widgets.hovered.weak_bg_fill = ACCENT_HOVER;
            widgets.active.weak_bg_fill = ACCENT_ACTIVE;

            let text = egui::RichText::new("清理后台进程")
                .font(bold(16.0))
                .color(egui::Color32::WHITE);
            if ui
                .add_sized([ui.available_width(), 44.0], egui::Button::new(text))
                .clicked()
            {
                let (msg, _) = release();
                *self.status.lock().unwrap() = msg;
            }
        });
        ui.add_space(14.0);
        let status = self.status.lock().unwrap().clone();
        ui.label(egui::RichText::new(status).size(13.0).color(MUTED));

        ui.add_space(14.0);
        let mut auto = self.auto.load(Ordering::Relaxed);
        let toggle = egui::RichText::new("自动清理（每 5 秒检查）").size(13.0).color(MUTED);
        if ui.checkbox(&mut auto, toggle).changed() {
            self.auto.store(auto, Ordering::Relaxed);
        }
    }
}

// 无边框窗口默认是直角的，亚克力会把整个矩形铺满，四角就会露出底色。
// 交给 DWM 把窗口本身裁成圆角，和面板的圆角对齐。
fn round_window(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE,
    };
    let round: i32 = 2; // DWMWCP_ROUND
    let no_border: u32 = 0xFFFF_FFFE; // DWMWA_COLOR_NONE
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &round as *const i32 as *const core::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &no_border as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

fn window_hwnd(cc: &eframe::CreationContext<'_>) -> Option<windows::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(w) => Some(windows::Win32::Foundation::HWND(
            w.hwnd.get() as *mut core::ffi::c_void
        )),
        _ => None,
    }
}

// 主屏正中交给 eframe 的 centered，它按逻辑点计算，高分屏也不会偏。
fn main() -> eframe::Result<()> {
    if std::env::args().any(|a| a == "--check") {
        // release 是 GUI 子系统，没有控制台时先挂上父进程的终端，否则 println 的输出会丢。
        unsafe {
            let _ = windows::Win32::System::Console::AttachConsole(
                windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
        println!("{}", report());
        return Ok(());
    }

    let (width, height) = (340.0, 230.0);
    let status = Arc::new(Mutex::new(peek()));
    let auto = Arc::new(AtomicBool::new(true));
    let options = eframe::NativeOptions {
        centered: true,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([width, height])
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_title("OringinFix"),
        ..Default::default()
    };
    eframe::run_native(
        "OringinFix",
        options,
        Box::new(move |cc| {
            configure(&cc.egui_ctx);
            // 浮动小窗优先 Transient Acrylic；Win11 不支持时退到浅色 Mica。
            let glass = window_vibrancy::apply_acrylic(cc, Some((255, 255, 255, 120))).is_ok()
                || window_vibrancy::apply_mica(cc, Some(false)).is_ok();
            if let Some(hwnd) = window_hwnd(cc) {
                round_window(hwnd);
            }

            // 后台巡检：勾上开关后每 5 秒检查一次，窗口最小化也照常工作，关窗即随进程结束。
            {
                let status = Arc::clone(&status);
                let auto = Arc::clone(&auto);
                let ctx = cc.egui_ctx.clone();
                std::thread::spawn(move || loop {
                    std::thread::sleep(Duration::from_secs(5));
                    // 勾了自动才真清理；状态文案始终刷新，好让自动化实例的提醒及时出现。
                    let msg = if auto.load(Ordering::Relaxed) {
                        release().0
                    } else {
                        peek()
                    };
                    let mut current = status.lock().unwrap();
                    if *current != msg {
                        *current = msg;
                        drop(current);
                        ctx.request_repaint();
                    }
                });
            }

            Ok(Box::new(App { status, glass, auto }))
        }),
    )
}

