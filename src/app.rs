//! GUI + 托盘。

use std::collections::HashSet;
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};
use windows::Win32::Graphics::Gdi::CreateSolidBrush;
use windows::Win32::UI::WindowsAndMessaging::{
    PostMessageW, SetForegroundWindow, ShowWindow, SW_HIDE, SW_RESTORE, SW_SHOW, WM_CLOSE,
};

use crate::config::{CloseBehavior, Config, Language, ThemeMode, resolve_dark};
use crate::fonts::{self, bold};
use crate::i18n::Tr;
use crate::theme::{self, Palette};
use crate::log::{self, LogEntry};
use crate::notify;
use crate::origin::{self, ProcKind, Snapshot};
use crate::startup;
use crate::update::{self, CheckResult};

const HINT_FADE_SECS: f64 = 1.8;

const TRAY_PNG: &[u8] = include_bytes!("../assets/tray.png");
const NAV_W: f32 = 148.0;
/// 主操作按钮统一尺寸
const BTN_W: f32 = 128.0;
const BTN_H: f32 = 40.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    Appearance,
    Settings,
    About,
}

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const URL_GITHUB: &str = "https://github.com/luckysus/OringinFix";
const URL_GITEE: &str = "https://gitee.com/neusu/OringinFix";
const URL_RELEASES: &str = "https://github.com/luckysus/OringinFix/releases";
const AUTHOR_LINE: &str = "su";

struct Shared {
    status: Mutex<String>,
    snap: Mutex<Snapshot>,
    protect: Mutex<Vec<String>>,
    auto: AtomicBool,
    interval: AtomicU64,
    start_with_windows: AtomicBool,
    close_to_tray: AtomicBool,
    silent_start: AtomicBool,
    notify_on_clean: AtomicBool,
    allow_exit: AtomicBool,
    hwnd: AtomicIsize,
    /// 显示窗口时置位，UI 侧刷新列表
    need_refresh: AtomicBool,
    /// 曾见过可见窗口的 Origin PID；用于区分「PPT 正在启动」和「编辑后关掉」。
    ever_windowed: Mutex<HashSet<u32>>,
    language: Mutex<Language>,
    theme: Mutex<ThemeMode>,
    /// UI 侧显示「已更新」闪烁
    flash_updated: AtomicBool,
    /// 当前是否深色（托盘唤起预填底色 / clear_color）
    dark_ui: AtomicBool,
}

pub struct App {
    shared: Arc<Shared>,
    glass: bool,
    page: Page,
    selected: HashSet<u32>,
    log_entries: Vec<LogEntry>,
    protect_text: String,
    _tray: Option<TrayIcon>,
    tray_auto: CheckMenuItem,
    tray_startup: CheckMenuItem,
    hint_until: Option<Instant>,
    last_dark: Option<bool>,
    update_rx: Option<Receiver<CheckResult>>,
    update_msg: Option<String>,
    update_url: Option<String>,
    update_checking: bool,
}

pub fn run(silent: bool) -> eframe::Result<()> {
    let cfg = Config::load();
    let start_with_windows = startup::is_enabled();
    let cfg = Config {
        start_with_windows,
        ..cfg
    };
    cfg.save();

    let shared = Arc::new(Shared {
        status: Mutex::new(String::new()),
        snap: Mutex::new(Snapshot::default()),
        protect: Mutex::new(cfg.protect_args.clone()),
        auto: AtomicBool::new(cfg.auto_clean),
        interval: AtomicU64::new(cfg.interval_secs.max(1)),
        start_with_windows: AtomicBool::new(start_with_windows),
        close_to_tray: AtomicBool::new(cfg.close_behavior == CloseBehavior::Tray),
        silent_start: AtomicBool::new(cfg.silent_start),
        notify_on_clean: AtomicBool::new(cfg.notify_on_clean),
        allow_exit: AtomicBool::new(false),
        hwnd: AtomicIsize::new(0),
        need_refresh: AtomicBool::new(true),
        ever_windowed: Mutex::new(HashSet::new()),
        language: Mutex::new(cfg.language),
        theme: Mutex::new(cfg.theme),
        flash_updated: AtomicBool::new(false),
        dark_ui: AtomicBool::new(resolve_dark(cfg.theme)),
    });
    refresh_shared(&shared);

    let hide_on_start = silent || cfg.silent_start && start_with_windows;

    let (width, height) = (620.0, 500.0);
    let options = eframe::NativeOptions {
        centered: true,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([width, height])
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_title("OringinFix")
            .with_visible(!hide_on_start),
        ..Default::default()
    };

    let shared_for_app = Arc::clone(&shared);
    let protect_text = cfg.protect_text();
    eframe::run_native(
        "OringinFix",
        options,
        Box::new(move |cc| {
            fonts::configure(&cc.egui_ctx);
            let dark0 = resolve_dark(cfg.theme);
            theme::apply(&cc.egui_ctx, dark0);
            // 亚克力色跟随主题，避免深色下托盘唤起先闪白
            let tint = acrylic_tint(dark0);
            let glass = window_vibrancy::apply_acrylic(cc, Some(tint)).is_ok()
                || window_vibrancy::apply_mica(cc, Some(dark0)).is_ok();
            if let Some(h) = window_hwnd(cc) {
                round_window(h);
                set_window_bg_brush(h, dark0);
                shared_for_app.hwnd.store(h.0 as isize, Ordering::SeqCst);
                if hide_on_start {
                    hide_window(&shared_for_app);
                }
            }

            {
                let shared = Arc::clone(&shared_for_app);
                let ctx = cc.egui_ctx.clone();
                std::thread::spawn(move || loop {
                    let secs = shared.interval.load(Ordering::Relaxed).max(1);
                    std::thread::sleep(Duration::from_secs(secs));
                    if shared.auto.load(Ordering::Relaxed) {
                        let protect = shared.protect.lock().unwrap().clone();
                        // 无窗口自动化（PPT 嵌入等）并入自动清理，不再单独开关
                        refresh_shared(&shared);
                        let before = shared.snap.lock().unwrap().clone();
                        let ever = shared.ever_windowed.lock().unwrap().clone();
                        let has_work = !before.stale_pids().is_empty()
                            || !before.auto_pids_old(&ever).is_empty();
                        if has_work {
                            let zh = matches!(
                                *shared.language.lock().unwrap(),
                                Language::Zh
                            );
                            let (msg, report) =
                                origin::release_with(&protect, true, &ever, zh);
                            apply_clean_result(&shared, msg, report);
                            ctx.request_repaint();
                        } else {
                            ctx.request_repaint();
                        }
                    } else {
                        refresh_shared(&shared);
                        ctx.request_repaint();
                    }
                });
            }

            let tray_tr = Tr::new(cfg.language);
            let tray_show = MenuItem::new(tray_tr.tray_show(), true, None);
            let tray_clean = MenuItem::new(tray_tr.tray_clean(), true, None);
            let tray_refresh = MenuItem::new(tray_tr.tray_refresh(), true, None);
            let tray_auto = CheckMenuItem::new(
                tray_tr.auto_clean(),
                true,
                shared_for_app.auto.load(Ordering::Relaxed),
                None,
            );
            let tray_startup = CheckMenuItem::new(
                tray_tr.startup(),
                true,
                shared_for_app.start_with_windows.load(Ordering::Relaxed),
                None,
            );
            let tray_quit = MenuItem::new(tray_tr.tray_quit(), true, None);

            let show_id = tray_show.id().as_ref().to_owned();
            let clean_id = tray_clean.id().as_ref().to_owned();
            let refresh_id = tray_refresh.id().as_ref().to_owned();
            let auto_id = tray_auto.id().as_ref().to_owned();
            let startup_id = tray_startup.id().as_ref().to_owned();
            let quit_id = tray_quit.id().as_ref().to_owned();

            let menu = Menu::new();
            let _ = menu.append(&tray_show);
            let _ = menu.append(&tray_clean);
            let _ = menu.append(&tray_refresh);
            let _ = menu.append(&PredefinedMenuItem::separator());
            let _ = menu.append(&tray_auto);
            let _ = menu.append(&tray_startup);
            let _ = menu.append(&PredefinedMenuItem::separator());
            let _ = menu.append(&tray_quit);

            {
                let shared = Arc::clone(&shared_for_app);
                std::thread::spawn(move || {
                    while let Ok(event) = MenuEvent::receiver().recv() {
                        let id = event.id.as_ref();
                        if id == show_id {
                            show_window(&shared);
                        } else if id == clean_id {
                            do_release_stale(&shared);
                        } else if id == refresh_id {
                            refresh_shared(&shared);
                            shared.need_refresh.store(true, Ordering::Relaxed);
                            shared.flash_updated.store(true, Ordering::Relaxed);
                        } else if id == auto_id {
                            let on = !shared.auto.load(Ordering::Relaxed);
                            shared.auto.store(on, Ordering::Relaxed);
                            persist(&shared);
                        } else if id == startup_id {
                            let on = !shared.start_with_windows.load(Ordering::Relaxed);
                            let silent = shared.silent_start.load(Ordering::Relaxed);
                            if startup::set_enabled(on, silent).is_ok() {
                                shared.start_with_windows.store(on, Ordering::Relaxed);
                                persist(&shared);
                            }
                        } else if id == quit_id {
                            request_exit(&shared);
                        }
                    }
                });
            }

            {
                let shared = Arc::clone(&shared_for_app);
                std::thread::spawn(move || {
                    while let Ok(event) = TrayIconEvent::receiver().recv() {
                        match event {
                            TrayIconEvent::DoubleClick { .. }
                            | TrayIconEvent::Click {
                                button: MouseButton::Left,
                                button_state: MouseButtonState::Up,
                                ..
                            } => show_window(&shared),
                            _ => {}
                        }
                    }
                });
            }

            let tray = load_tray_icon().and_then(|icon| {
                TrayIconBuilder::new()
                    .with_menu(Box::new(menu))
                    .with_menu_on_left_click(false)
                    .with_tooltip("OringinFix")
                    .with_icon(icon)
                    .build()
                    .ok()
            });

            Ok(Box::new(App {
                shared: shared_for_app,
                glass,
                page: Page::Home,
                selected: HashSet::new(),
                log_entries: log::load(),
                protect_text,
                _tray: tray,
                tray_auto,
                tray_startup,
                hint_until: None,
                last_dark: Some(resolve_dark(cfg.theme)),
                update_rx: None,
                update_msg: None,
                update_url: None,
                update_checking: false,
            }))
        }),
    )
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // 与主题底色一致，避免透明窗口 ShowWindow 时先露出系统白底
        if self.shared.dark_ui.load(Ordering::Relaxed) {
            [28.0 / 255.0, 28.0 / 255.0, 32.0 / 255.0, 1.0]
        } else {
            [252.0 / 255.0, 252.0 / 255.0, 254.0 / 255.0, 1.0]
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.tray_auto
            .set_checked(self.shared.auto.load(Ordering::Relaxed));
        self.tray_startup
            .set_checked(self.shared.start_with_windows.load(Ordering::Relaxed));

        if self.shared.flash_updated.swap(false, Ordering::Relaxed) {
            self.hint_until =
                Some(Instant::now() + Duration::from_secs_f64(HINT_FADE_SECS));
        }

        let lang = *self.shared.language.lock().unwrap();
        let theme_mode = *self.shared.theme.lock().unwrap();
        let tr = Tr::new(lang);
        let dark = resolve_dark(theme_mode);
        self.shared.dark_ui.store(dark, Ordering::Relaxed);
        if self.last_dark != Some(dark) {
            theme::apply(ui.ctx(), dark);
            if let Some(h) = hwnd_of(&self.shared) {
                set_window_bg_brush(h, dark);
            }
            if self.glass {
                let _ = window_vibrancy::apply_acrylic(frame, Some(acrylic_tint(dark)));
            }
            self.last_dark = Some(dark);
        }
        let palette = Palette::for_dark(dark);

        if self.shared.need_refresh.swap(false, Ordering::Relaxed) {
            // 只丢掉已经不存在的 PID，保留用户勾选。
            let live: HashSet<u32> = self
                .shared
                .snap
                .lock()
                .unwrap()
                .procs
                .iter()
                .map(|p| p.pid)
                .collect();
            self.selected.retain(|pid| live.contains(pid));
            self.log_entries = log::load();
        }

        if ui.input(|i| i.viewport().close_requested()) {
            if self.shared.allow_exit.load(Ordering::Relaxed)
                || !self.shared.close_to_tray.load(Ordering::Relaxed)
            {
                self.shared.allow_exit.store(true, Ordering::Relaxed);
            } else {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                hide_window(&self.shared);
            }
        }

        let rect = ui.max_rect();
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, palette.fill(self.glass));

        let mut root =
            ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(16.0, 14.0))));

        paint_title_bar(&mut root, &self.shared, palette);
        root.add_space(10.0);

        let body = root.available_rect_before_wrap();
        let nav_rect = egui::Rect::from_min_size(body.min, egui::vec2(NAV_W, body.height()));
        let content_rect =
            egui::Rect::from_min_max(egui::pos2(nav_rect.max.x + 16.0, body.min.y), body.max);

        root.painter()
            .rect_filled(nav_rect, egui::CornerRadius::same(12), palette.nav_bg());

        let mut nav = root.new_child(egui::UiBuilder::new().max_rect(nav_rect.shrink(8.0)));
        nav.vertical(|ui| {
            ui.add_space(4.0);
            if nav_item(ui, tr.home(), self.page == Page::Home, palette) {
                self.page = Page::Home;
            }
            ui.add_space(4.0);
            if nav_item(ui, tr.appearance(), self.page == Page::Appearance, palette) {
                self.page = Page::Appearance;
            }
            ui.add_space(4.0);
            if nav_item(ui, tr.settings(), self.page == Page::Settings, palette) {
                self.page = Page::Settings;
            }
            ui.add_space(4.0);
            if nav_item(ui, tr.about(), self.page == Page::About, palette) {
                self.page = Page::About;
            }
        });

        let mut content = root.new_child(egui::UiBuilder::new().max_rect(content_rect));
        match self.page {
            Page::Home => self.ui_home(&mut content, tr, palette),
            Page::Appearance => self.ui_appearance(&mut content, tr, palette),
            Page::Settings => self.ui_settings(&mut content, tr, palette),
            Page::About => self.ui_about(&mut content, tr, palette),
        }

        self.poll_update_check(tr);
        if self.update_checking {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
        if self
            .hint_until
            .is_some_and(|u| Instant::now() < u)
        {
            ui.ctx().request_repaint();
        }
    }
}

impl App {
    fn ui_home(&mut self, ui: &mut egui::Ui, tr: Tr, palette: Palette) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.spacing_mut().item_spacing.y = 8.0;

            if equal_primary(ui, tr.clean_stale(), palette) {
                do_release_stale(&self.shared);
                self.selected.clear();
                self.log_entries = log::load();
            }
            if equal_secondary(ui, tr.refresh(), palette) {
                refresh_shared(&self.shared);
                self.selected.clear();
                self.hint_until =
                    Some(Instant::now() + Duration::from_secs_f64(HINT_FADE_SECS));
            }
            if equal_secondary(ui, tr.clean_auto(), palette) {
                do_release_auto(&self.shared);
                self.selected.clear();
                self.log_entries = log::load();
            }
            if !self.selected.is_empty() {
                let pids: Vec<u32> = self.selected.iter().copied().collect();
                let snap = self.shared.snap.lock().unwrap();
                let mem = snap.memory_of(&pids);
                drop(snap);
                if equal_primary(ui, tr.clean_selected(), palette) {
                    let zh = matches!(tr.lang, Language::Zh);
                    let (msg, report) = origin::release_selected(&pids, true, zh);
                    let msg = if mem > 0 && report.killed > 0 {
                        if zh {
                            format!("{msg}（约 {}）", origin::format_bytes(mem))
                        } else {
                            format!("{msg} (~{})", origin::format_bytes(mem))
                        }
                    } else {
                        msg
                    };
                    apply_clean_result(&self.shared, msg, report);
                    self.selected.clear();
                    self.log_entries = log::load();
                }
            }
        });

        ui.add_space(10.0);
        let status = self.shared.status.lock().unwrap().clone();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(status).size(14.0).color(palette.muted));
            if let Some(until) = self.hint_until {
                let now = Instant::now();
                if now >= until {
                    self.hint_until = None;
                } else {
                    let fade = until.duration_since(now).as_secs_f64() / HINT_FADE_SECS;
                    let alpha = fade.clamp(0.0, 1.0) as f32;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(tr.updated())
                                .size(13.0)
                                .color(palette.muted.gamma_multiply(alpha)),
                        );
                    });
                }
            }
        });

        let snap = self.shared.snap.lock().unwrap().clone();
        let stale_mem = snap.memory_of(&snap.stale_pids());
        if stale_mem > 0 {
            ui.label(
                egui::RichText::new(tr.stale_mem(&origin::format_bytes(stale_mem)))
                    .size(13.5)
                    .color(palette.muted),
            );
        }

        let oldest_stale_pid = snap
            .by_kind(ProcKind::Stale)
            .filter_map(|p| p.age_secs.map(|a| (a, p.pid)))
            .max_by_key(|(a, _)| *a)
            .map(|(_, pid)| pid);

        ui.add_space(8.0);
        // 进程列表限高，把剩余高度留给下方日志
        egui::ScrollArea::vertical()
            .max_height(120.0)
            .id_salt("proc_list")
            .show(ui, |ui| {
                if snap.is_idle() {
                    ui.label(
                        egui::RichText::new(tr.no_procs())
                            .size(13.5)
                            .color(palette.muted),
                    );
                }
                for kind in [
                    ProcKind::Stale,
                    ProcKind::Windowed,
                    ProcKind::Automation,
                    ProcKind::Protected,
                ] {
                    let rows: Vec<_> = snap.by_kind(kind).cloned().collect();
                    if rows.is_empty() {
                        continue;
                    }
                    ui.label(
                        egui::RichText::new(kind_label(tr, kind))
                            .size(13.5)
                            .color(palette.muted),
                    );
                    for p in rows {
                        let selectable =
                            matches!(kind, ProcKind::Stale | ProcKind::Automation | ProcKind::Protected);
                        let is_oldest =
                            kind == ProcKind::Stale && oldest_stale_pid == Some(p.pid);
                        let mut paint_row = |ui: &mut egui::Ui| {
                            let age = p
                                .age_secs
                                .map(|a| format!("{a}s"))
                                .unwrap_or_else(|| "?".into());
                            let young = if p.young {
                                format!(" · {}", tr.just_started())
                            } else {
                                String::new()
                            };
                            let oldest = if is_oldest {
                                format!(" · {}", tr.oldest())
                            } else {
                                String::new()
                            };
                            let ole = p
                                .ole_source
                                .as_ref()
                                .map(|s| format!(" · {s}"))
                                .unwrap_or_default();
                            let line = format!(
                                "PID {} · {} · {age}{young}{oldest}{ole}",
                                p.pid,
                                origin::format_bytes(p.memory_bytes)
                            );
                            ui.horizontal(|ui| {
                                if selectable {
                                    let mut on = self.selected.contains(&p.pid);
                                    ui.checkbox(
                                        &mut on,
                                        egui::RichText::new(&line).size(14.0).color(palette.text),
                                    );
                                    if on {
                                        self.selected.insert(p.pid);
                                    } else {
                                        self.selected.remove(&p.pid);
                                    }
                                } else {
                                    ui.add_space(18.0);
                                    ui.label(
                                        egui::RichText::new(line).size(14.0).color(palette.text),
                                    );
                                }
                            });
                        };
                        if is_oldest {
                            egui::Frame::new()
                                .fill(palette.highlight)
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(4, 2))
                                .show(ui, paint_row);
                        } else {
                            paint_row(ui);
                        }
                    }
                    ui.add_space(4.0);
                }
            });

        ui.add_space(8.0);
        ui.label(egui::RichText::new(tr.recent()).size(13.5).color(palette.muted));
        ui.add_space(4.0);
        let log_h = ui.available_height().max(100.0);
        egui::ScrollArea::vertical()
            .max_height(log_h)
            .auto_shrink([false, false])
            .id_salt("clean_log")
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if self.log_entries.is_empty() {
                    ui.label(egui::RichText::new(tr.no_log()).size(13.5).color(palette.muted));
                } else {
                    let zh = matches!(tr.lang, Language::Zh);
                    for e in &self.log_entries {
                        let msg = localize_log_message(&e.message, zh);
                        ui.label(
                            egui::RichText::new(format!("{}  {msg}", e.time))
                                .size(13.0)
                                .color(palette.muted),
                        );
                    }
                }
            });
    }

    fn ui_appearance(&mut self, ui: &mut egui::Ui, tr: Tr, palette: Palette) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(
                egui::RichText::new(tr.language())
                    .size(14.5)
                    .color(palette.text),
            );
            ui.add_space(6.0);
            let mut lang = *self.shared.language.lock().unwrap();
            if segment_row(
                ui,
                palette,
                &mut lang,
                &[
                    (Language::Zh, tr.lang_zh()),
                    (Language::En, tr.lang_en()),
                ],
            ) {
                *self.shared.language.lock().unwrap() = lang;
                persist(&self.shared);
                refresh_shared(&self.shared);
            }

            ui.add_space(18.0);
            ui.label(
                egui::RichText::new(tr.theme())
                    .size(14.5)
                    .color(palette.text),
            );
            ui.add_space(6.0);
            let mut theme = *self.shared.theme.lock().unwrap();
            if theme_cards(ui, palette, &mut theme, tr) {
                *self.shared.theme.lock().unwrap() = theme;
                persist(&self.shared);
            }
        });
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui, tr: Tr, palette: Palette) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut auto = self.shared.auto.load(Ordering::Relaxed);
            if ui
                .checkbox(
                    &mut auto,
                    egui::RichText::new(tr.auto_clean()).size(14.5).color(palette.text),
                )
                .changed()
            {
                self.shared.auto.store(auto, Ordering::Relaxed);
                self.tray_auto.set_checked(auto);
                persist(&self.shared);
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tr.interval()).size(14.5).color(palette.text));
                let mut secs = self.shared.interval.load(Ordering::Relaxed) as f32;
                if ui
                    .add(
                        egui::Slider::new(&mut secs, 3.0..=60.0)
                            .suffix(tr.seconds())
                            .clamping(egui::SliderClamping::Always),
                    )
                    .changed()
                {
                    self.shared.interval.store(secs as u64, Ordering::Relaxed);
                    persist(&self.shared);
                }
            });

            ui.add_space(8.0);
            let mut notify = self.shared.notify_on_clean.load(Ordering::Relaxed);
            if ui
                .checkbox(
                    &mut notify,
                    egui::RichText::new(tr.notify()).size(14.5).color(palette.text),
                )
                .changed()
            {
                self.shared.notify_on_clean.store(notify, Ordering::Relaxed);
                persist(&self.shared);
            }

            ui.add_space(8.0);
            let mut silent = self.shared.silent_start.load(Ordering::Relaxed);
            if ui
                .checkbox(
                    &mut silent,
                    egui::RichText::new(tr.silent()).size(14.5).color(palette.text),
                )
                .changed()
            {
                self.shared.silent_start.store(silent, Ordering::Relaxed);
                if self.shared.start_with_windows.load(Ordering::Relaxed) {
                    let _ = startup::set_enabled(true, silent);
                }
                persist(&self.shared);
            }

            ui.add_space(8.0);
            let mut startup_on = self.shared.start_with_windows.load(Ordering::Relaxed);
            if ui
                .checkbox(
                    &mut startup_on,
                    egui::RichText::new(tr.startup()).size(14.5).color(palette.text),
                )
                .changed()
            {
                let silent = self.shared.silent_start.load(Ordering::Relaxed);
                match startup::set_enabled(startup_on, silent) {
                    Ok(()) => {
                        self.shared
                            .start_with_windows
                            .store(startup_on, Ordering::Relaxed);
                        self.tray_startup.set_checked(startup_on);
                        persist(&self.shared);
                    }
                    Err(e) => {
                        *self.shared.status.lock().unwrap() = if matches!(tr.lang, Language::Zh) {
                            format!("开机自启设置失败：{e}")
                        } else {
                            format!("Failed to set startup: {e}")
                        };
                    }
                }
            }

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(tr.close_when())
                        .size(14.5)
                        .color(palette.text),
                );
                ui.add_space(12.0);
                let mut to_tray = self.shared.close_to_tray.load(Ordering::Relaxed);
                if ui
                    .radio_value(
                        &mut to_tray,
                        true,
                        egui::RichText::new(tr.to_tray()).size(14.5).color(palette.text),
                    )
                    .changed()
                {
                    self.shared.close_to_tray.store(true, Ordering::Relaxed);
                    persist(&self.shared);
                }
                ui.add_space(8.0);
                if ui
                    .radio_value(
                        &mut to_tray,
                        false,
                        egui::RichText::new(tr.exit_app()).size(14.5).color(palette.text),
                    )
                    .changed()
                {
                    self.shared.close_to_tray.store(false, Ordering::Relaxed);
                    persist(&self.shared);
                }
            });

            ui.add_space(14.0);
            ui.label(
                egui::RichText::new(tr.protect())
                    .size(13.5)
                    .color(palette.muted),
            );
            ui.add_space(4.0);
            let resp = ui.scope(|ui| {
                ui.visuals_mut().extreme_bg_color = palette.input_bg();
                ui.visuals_mut().widgets.inactive.bg_stroke =
                    egui::Stroke::new(1.0, palette.input_stroke());
                ui.visuals_mut().widgets.hovered.bg_stroke =
                    egui::Stroke::new(1.0, palette.selected_stroke());
                ui.visuals_mut().widgets.active.bg_stroke =
                    egui::Stroke::new(1.5, palette.selected_stroke());
                ui.add(
                    egui::TextEdit::singleline(&mut self.protect_text)
                        .desired_width(ui.available_width())
                        .text_color(palette.text)
                        .hint_text(
                            egui::RichText::new(tr.protect_hint()).color(palette.muted),
                        ),
                )
            })
            .inner;
            if resp.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let mut cfg_protect = Config::default();
                cfg_protect.set_protect_text(&self.protect_text);
                *self.shared.protect.lock().unwrap() = cfg_protect.protect_args.clone();
                self.protect_text = cfg_protect.protect_text();
                persist(&self.shared);
                refresh_shared(&self.shared);
            }
        });
    }

    fn start_update_check(&mut self) {
        if self.update_checking {
            return;
        }
        self.update_checking = true;
        self.update_msg = None;
        self.update_url = None;
        let (tx, rx) = mpsc::channel();
        self.update_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(update::check_latest(APP_VERSION));
        });
    }

    fn poll_update_check(&mut self, tr: Tr) {
        let Some(rx) = self.update_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.update_rx = None;
                self.update_checking = false;
                match result {
                    CheckResult::UpToDate { latest } => {
                        self.update_msg = Some(tr.about_up_to_date(&latest));
                        self.update_url = None;
                    }
                    CheckResult::Available { latest, url } => {
                        self.update_msg = Some(tr.about_update_available(&latest));
                        self.update_url = Some(url);
                    }
                    CheckResult::NoRelease => {
                        self.update_msg = Some(tr.about_no_release().to_owned());
                        self.update_url = Some(URL_RELEASES.to_string());
                    }
                    CheckResult::Failed(err) => {
                        self.update_msg = Some(tr.about_check_failed(&err));
                        self.update_url = Some(URL_RELEASES.to_string());
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.update_rx = None;
                self.update_checking = false;
                self.update_msg = Some(tr.about_check_failed("disconnected"));
            }
        }
    }

    fn ui_about(&mut self, ui: &mut egui::Ui, tr: Tr, palette: Palette) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(
                egui::RichText::new("OringinFix")
                    .font(bold(22.0))
                    .color(palette.text),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(tr.about_tagline())
                    .size(15.0)
                    .color(palette.muted),
            );

            ui.add_space(16.0);
            {
                let row_h = 30.0;
                let update_msg = self.update_msg.clone();
                let update_url = self.update_url.clone();
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), row_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_height(row_h);
                        ui.set_height(row_h);
                        ui.spacing_mut().item_spacing.x = 10.0;
                        ui.label(
                            egui::RichText::new(format!(
                                "{}  v{APP_VERSION}",
                                tr.about_version()
                            ))
                            .size(14.5)
                            .color(palette.text),
                        );
                        if self.update_checking {
                            ui.label(
                                egui::RichText::new(tr.about_checking())
                                    .size(13.5)
                                    .color(palette.muted),
                            );
                        } else if link_chip(ui, tr.about_check_update(), palette) {
                            self.start_update_check();
                        }
                        if let Some(msg) = &update_msg {
                            ui.label(
                                egui::RichText::new(msg.as_str())
                                    .size(13.5)
                                    .color(palette.muted),
                            );
                        }
                        if let Some(url) = &update_url {
                            if link_chip(ui, tr.about_open_release(), palette) {
                                open_url(url);
                            }
                        }
                    },
                );
            }

            ui.add_space(16.0);
            ui.label(
                egui::RichText::new(tr.about_author())
                    .size(13.5)
                    .color(palette.muted),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(AUTHOR_LINE)
                    .size(14.5)
                    .color(palette.text),
            );

            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(tr.about_license())
                    .size(13.5)
                    .color(palette.muted),
            );
            ui.add_space(2.0);
            ui.label(egui::RichText::new("MIT").size(14.5).color(palette.text));

            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(tr.about_repo())
                    .size(13.5)
                    .color(palette.muted),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if link_chip(ui, "GitHub", palette) {
                    open_url(URL_GITHUB);
                }
                if link_chip(ui, "Gitee", palette) {
                    open_url(URL_GITEE);
                }
            });

            ui.add_space(18.0);
            ui.label(
                egui::RichText::new(tr.about_thanks())
                    .size(12.5)
                    .color(palette.muted),
            );
        });
    }
}

fn open_url(url: &str) {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .spawn();
}

fn link_chip(ui: &mut egui::Ui, label: &str, palette: Palette) -> bool {
    let font = egui::FontId::proportional(13.5);
    let text_w = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), palette.text)
        .size()
        .x;
    let pad_x = 12.0;
    let h = 28.0;
    let w = (text_w + pad_x * 2.0).max(72.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);

    let fill = if resp.is_pointer_button_down_on() {
        palette.secondary_active()
    } else if resp.hovered() {
        palette.secondary_hover()
    } else {
        palette.secondary_fill()
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), fill);
    // 文字几何居中，避免 Button 内边距把中文顶偏
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        palette.text,
    );
    resp.clicked()
}

/// 历史日志可能是中文；英文界面下尽量对照翻译常见句式。
fn localize_log_message(msg: &str, zh: bool) -> String {
    if zh {
        return msg.to_owned();
    }
    let mut out = msg.to_owned();
    let pairs = [
        ("已清理 ", "Cleaned "),
        (" 个自动化实例", " automation instance(s)"),
        (" 个进程", " process(es)"),
        ("，约释放 ", ", freed ~"),
        ("约释放 ", "freed ~"),
        (" 个权限不足", " access denied"),
        (" 个失败", " failed"),
        ("跳过 ", "skipped "),
        (" 个刚启动的", " just-started"),
        ("没有可清理的自动化实例", "No automation instances to clean"),
        ("没有可清理的进程", "Nothing to clean"),
        ("Origin 未在运行", "Origin is not running"),
        ("进程刚启动，暂未处理", "Process just started; skipped"),
    ];
    for (zh_s, en_s) in pairs {
        out = out.replace(zh_s, en_s);
    }
    out
}

fn kind_label(tr: Tr, kind: ProcKind) -> &'static str {
    match kind {
        ProcKind::Stale => tr.kind_stale(),
        ProcKind::Windowed => tr.kind_windowed(),
        ProcKind::Automation => tr.kind_auto(),
        ProcKind::Protected => tr.kind_protected(),
    }
}

fn refresh_shared(shared: &Shared) {
    let protect = shared.protect.lock().unwrap().clone();
    let snap = origin::snapshot(&protect);
    {
        let mut ever = shared.ever_windowed.lock().unwrap();
        for p in &snap.procs {
            if p.kind == origin::ProcKind::Windowed {
                ever.insert(p.pid);
            }
        }
        // 进程已不在则丢掉记录，避免 PID 复用误判。
        let live: HashSet<u32> = snap.procs.iter().map(|p| p.pid).collect();
        ever.retain(|pid| live.contains(pid));
    }
    let zh = matches!(*shared.language.lock().unwrap(), Language::Zh);
    *shared.status.lock().unwrap() = origin::format_status(&snap, zh);
    *shared.snap.lock().unwrap() = snap;
}

fn apply_clean_result(shared: &Shared, msg: String, report: origin::CleanReport) {
    if report.killed > 0 {
        log::append(&msg);
        if shared.notify_on_clean.load(Ordering::Relaxed) {
            notify::toast("OringinFix", &msg);
        }
    }

    // 重新扫描，但已确认杀掉的 PID 绝不再显示（避免 Toolhelp 短暂滞后又刷回来）。
    let protect = shared.protect.lock().unwrap().clone();
    let mut fresh = origin::snapshot(&protect);
    if !report.killed_pids.is_empty() {
        fresh
            .procs
            .retain(|p| !report.killed_pids.contains(&p.pid));
    }
    *shared.snap.lock().unwrap() = fresh;
    *shared.status.lock().unwrap() = msg;
    shared.need_refresh.store(true, Ordering::Relaxed);
    shared.flash_updated.store(true, Ordering::Relaxed);
}

fn do_release_stale(shared: &Shared) {
    let protect = shared.protect.lock().unwrap().clone();
    let ever = shared.ever_windowed.lock().unwrap().clone();
    let zh = matches!(*shared.language.lock().unwrap(), Language::Zh);
    let (msg, report) = origin::release_with(&protect, true, &ever, zh);
    apply_clean_result(shared, msg, report);
}

fn do_release_auto(shared: &Shared) {
    let protect = shared.protect.lock().unwrap().clone();
    let zh = matches!(*shared.language.lock().unwrap(), Language::Zh);
    let (msg, report) = origin::release_auto(&protect, zh);
    apply_clean_result(shared, msg, report);
}

fn persist(shared: &Shared) {
    Config {
        auto_clean: shared.auto.load(Ordering::Relaxed),
        interval_secs: shared.interval.load(Ordering::Relaxed),
        start_with_windows: shared.start_with_windows.load(Ordering::Relaxed),
        close_behavior: if shared.close_to_tray.load(Ordering::Relaxed) {
            CloseBehavior::Tray
        } else {
            CloseBehavior::Exit
        },
        silent_start: shared.silent_start.load(Ordering::Relaxed),
        notify_on_clean: shared.notify_on_clean.load(Ordering::Relaxed),
        auto_clean_embedding: true,
        protect_args: shared.protect.lock().unwrap().clone(),
        language: *shared.language.lock().unwrap(),
        theme: *shared.theme.lock().unwrap(),
    }
    .save();
}

fn paint_title_bar(ui: &mut egui::Ui, shared: &Shared, palette: Palette) {
    let (header, drag) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::click_and_drag());
    if drag.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    ui.painter().text(
        header.left_center(),
        egui::Align2::LEFT_CENTER,
        "OringinFix",
        bold(18.5),
        palette.text,
    );
    let btn = 28.0;
    let close_id = ui.scope_id().with("close");
    let min_id = ui.scope_id().with("min");
    let close_rect = egui::Rect::from_center_size(
        header.right_center() - egui::vec2(btn / 2.0, 0.0),
        egui::vec2(btn, btn),
    );
    if title_button(ui, close_rect, "×", close_id, palette) {
        if shared.close_to_tray.load(Ordering::Relaxed) {
            hide_window(shared);
        } else {
            request_exit(shared);
        }
    }
    let min_rect = egui::Rect::from_center_size(
        header.right_center() - egui::vec2(btn / 2.0 + btn + 8.0, 0.0),
        egui::vec2(btn, btn),
    );
    if title_button(ui, min_rect, "−", min_id, palette) {
        // 托盘工具：最小化也进托盘，避免任务栏留「最小化」且点不回来
        hide_window(shared);
    }
}

fn nav_item(ui: &mut egui::Ui, label: &str, active: bool, palette: Palette) -> bool {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 36.0), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hover_overlay = if palette.dark {
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 12)
    } else {
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 10)
    };
    let bg = if active {
        palette.nav_active()
    } else if resp.hovered() {
        hover_overlay
    } else {
        egui::Color32::TRANSPARENT
    };
    if bg != egui::Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), bg);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        if active {
            bold(15.5)
        } else {
            egui::FontId::proportional(15.5)
        },
        if active { palette.text } else { palette.muted },
    );
    resp.clicked()
}

fn hwnd_of(shared: &Shared) -> Option<HWND> {
    let raw = shared.hwnd.load(Ordering::SeqCst);
    if raw == 0 {
        None
    } else {
        Some(HWND(raw as *mut core::ffi::c_void))
    }
}

fn cloak_window(hwnd: HWND, cloak: bool) {
    let v: i32 = if cloak { 1 } else { 0 };
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CLOAK,
            &v as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&v) as u32,
        );
    }
}

fn hide_window(shared: &Shared) {
    if let Some(hwnd) = hwnd_of(shared) {
        unsafe {
            // 必须 SW_HIDE，任务栏按钮才会消失；仅 Cloak 会留下「最小化」且点不回
            cloak_window(hwnd, false);
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

fn acrylic_tint(dark: bool) -> (u8, u8, u8, u8) {
    if dark {
        (28, 28, 32, 200)
    } else {
        (252, 252, 254, 160)
    }
}

fn theme_colorref(dark: bool) -> windows::Win32::Foundation::COLORREF {
    let (r, g, b) = if dark {
        (28u8, 28, 32)
    } else {
        (252, 252, 254)
    };
    windows::Win32::Foundation::COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16))
}

fn set_window_bg_brush(hwnd: HWND, dark: bool) {
    unsafe {
        let brush = CreateSolidBrush(theme_colorref(dark));
        let _ = windows::Win32::UI::WindowsAndMessaging::SetClassLongPtrW(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::GCLP_HBRBACKGROUND,
            brush.0 as isize,
        );
    }
}

fn show_window(shared: &Shared) {
    refresh_shared(shared);
    shared.need_refresh.store(true, Ordering::Relaxed);
    if let Some(hwnd) = hwnd_of(shared) {
        let dark = shared.dark_ui.load(Ordering::Relaxed);
        set_window_bg_brush(hwnd, dark);
        unsafe {
            cloak_window(hwnd, false);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

fn request_exit(shared: &Shared) {
    shared.allow_exit.store(true, Ordering::Relaxed);
    if let Some(hwnd) = hwnd_of(shared) {
        cloak_window(hwnd, false);
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    } else {
        std::process::exit(0);
    }
}

/// 横排分段选择；返回是否变更。
fn segment_row<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    palette: Palette,
    value: &mut T,
    options: &[(T, &str)],
) -> bool {
    let n = options.len().max(1) as f32;
    let gap = 6.0;
    let total = ui.available_width();
    let w = ((total - gap * (n - 1.0)) / n).max(72.0);
    let h = 34.0;
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for &(opt, label) in options {
            let selected = *value == opt;
            let fill = if selected {
                palette.selected_fill()
            } else {
                palette.secondary_fill()
            };
            let stroke = if selected {
                egui::Stroke::new(1.5, palette.selected_stroke())
            } else {
                egui::Stroke::NONE
            };
            let text = egui::RichText::new(label).size(13.5).color(palette.text);
            let resp = ui.add_sized(
                [w, h],
                egui::Button::new(text)
                    .fill(fill)
                    .corner_radius(egui::CornerRadius::same(8))
                    .stroke(stroke),
            );
            if resp.clicked() && !selected {
                *value = opt;
                changed = true;
            }
        }
    });
    changed
}

/// 外观三卡片：迷你窗口预览 + 文案。
fn theme_cards(ui: &mut egui::Ui, palette: Palette, theme: &mut ThemeMode, tr: Tr) -> bool {
    let gap = 8.0;
    let total = ui.available_width();
    let w = ((total - gap * 2.0) / 3.0).max(100.0);
    let h = 98.0;
    let mut changed = false;

    let modes = [
        (ThemeMode::System, tr.theme_system()),
        (ThemeMode::Light, tr.theme_light()),
        (ThemeMode::Dark, tr.theme_dark()),
    ];

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (mode, label) in modes {
            let selected = *theme == mode;
            let hovered_fill = if palette.dark {
                egui::Color32::from_rgb(68, 68, 74)
            } else {
                egui::Color32::from_rgb(238, 238, 242)
            };

            let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            let fill = if selected {
                palette.selected_fill()
            } else if resp.hovered() {
                hovered_fill
            } else {
                palette.secondary_fill()
            };

            let painter = ui.painter();
            painter.rect_filled(rect, egui::CornerRadius::same(12), fill);
            if selected {
                painter.rect_stroke(
                    rect,
                    egui::CornerRadius::same(12),
                    egui::Stroke::new(1.5, palette.selected_stroke()),
                    egui::StrokeKind::Inside,
                );
            }

            let win = egui::Rect::from_center_size(
                egui::pos2(rect.center().x, rect.min.y + 36.0),
                egui::vec2((w - 18.0).min(112.0), 48.0),
            );
            paint_theme_preview(painter, win, mode);

            let label_color = if selected {
                palette.text
            } else {
                palette.muted
            };
            painter.text(
                egui::pos2(rect.center().x, rect.max.y - 14.0),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(13.0),
                label_color,
            );

            if resp.clicked() && !selected {
                *theme = mode;
                changed = true;
            }
        }
    });
    changed
}

fn paint_theme_preview(painter: &egui::Painter, win: egui::Rect, mode: ThemeMode) {
    let r = egui::CornerRadius::same(6);

    match mode {
        ThemeMode::System => {
            // 左右对半：浅 | 深
            let mid = win.center().x;
            let left = egui::Rect::from_min_max(win.min, egui::pos2(mid, win.max.y));
            let right = egui::Rect::from_min_max(egui::pos2(mid, win.min.y), win.max);
            clip_round_half(painter, win, left, egui::Color32::from_rgb(244, 245, 248), true);
            clip_round_half(painter, win, right, egui::Color32::from_rgb(32, 32, 36), false);
            paint_mini_chrome(
                painter,
                left,
                egui::Color32::from_rgb(232, 233, 238),
                egui::Color32::from_rgb(200, 200, 208),
                egui::Color32::from_rgb(55, 55, 60),
                egui::Color32::from_rgb(170, 170, 178),
                true,
            );
            paint_mini_chrome(
                painter,
                right,
                egui::Color32::from_rgb(42, 42, 48),
                egui::Color32::from_rgb(58, 58, 66),
                egui::Color32::from_rgb(220, 220, 228),
                egui::Color32::from_rgb(120, 120, 130),
                false,
            );
            // 中缝
            painter.line_segment(
                [egui::pos2(mid, win.min.y + 1.0), egui::pos2(mid, win.max.y - 1.0)],
                egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 90)),
            );
            painter.rect_stroke(
                win,
                r,
                egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(0, 0, 0, 40)),
                egui::StrokeKind::Outside,
            );
        }
        ThemeMode::Light => {
            painter.rect_filled(win, r, egui::Color32::from_rgb(248, 249, 252));
            paint_mini_chrome(
                painter,
                win,
                egui::Color32::from_rgb(255, 255, 255),
                egui::Color32::from_rgb(228, 229, 234),
                egui::Color32::from_rgb(35, 35, 40),
                egui::Color32::from_rgb(160, 160, 168),
                true,
            );
            painter.rect_stroke(
                win,
                r,
                egui::Stroke::new(1.0, egui::Color32::from_rgb(210, 212, 218)),
                egui::StrokeKind::Outside,
            );
        }
        ThemeMode::Dark => {
            painter.rect_filled(win, r, egui::Color32::from_rgb(28, 28, 32));
            paint_mini_chrome(
                painter,
                win,
                egui::Color32::from_rgb(38, 38, 44),
                egui::Color32::from_rgb(52, 52, 60),
                egui::Color32::from_rgb(235, 235, 240),
                egui::Color32::from_rgb(130, 130, 140),
                true,
            );
            painter.rect_stroke(
                win,
                r,
                egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 28)),
                egui::StrokeKind::Outside,
            );
        }
    }
}

fn clip_round_half(
    painter: &egui::Painter,
    full: egui::Rect,
    half: egui::Rect,
    color: egui::Color32,
    left: bool,
) {
    let mut radii = egui::CornerRadius::ZERO;
    if left {
        radii.nw = 6;
        radii.sw = 6;
    } else {
        radii.ne = 6;
        radii.se = 6;
    }
    let _ = full;
    painter.rect_filled(half, radii, color);
}

/// 迷你侧栏 + 标题条 + 内容行（贴合本应用布局）。
fn paint_mini_chrome(
    painter: &egui::Painter,
    area: egui::Rect,
    chrome: egui::Color32,
    nav: egui::Color32,
    accent: egui::Color32,
    muted: egui::Color32,
    full_round: bool,
) {
    let pad = 4.0;
    let inner = area.shrink(pad);
    if inner.width() < 8.0 || inner.height() < 8.0 {
        return;
    }

    let title_h = 7.0;
    let title = egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), title_h));
    painter.rect_filled(
        title,
        if full_round {
            egui::CornerRadius {
                nw: 3,
                ne: 3,
                sw: 0,
                se: 0,
            }
        } else {
            egui::CornerRadius::ZERO
        },
        chrome,
    );
    // 窗口控制点
    let dot_y = title.center().y;
    for (i, c) in [
        egui::Color32::from_rgb(255, 95, 86),
        egui::Color32::from_rgb(255, 189, 46),
        egui::Color32::from_rgb(39, 201, 63),
    ]
    .into_iter()
    .enumerate()
    {
        let x = title.max.x - 5.0 - i as f32 * 5.5;
        painter.circle_filled(egui::pos2(x, dot_y), 1.4, c);
    }

    let body = egui::Rect::from_min_max(
        egui::pos2(inner.min.x, title.max.y + 2.0),
        inner.max,
    );
    let nav_w = (body.width() * 0.28).clamp(8.0, 16.0);
    let nav_rect = egui::Rect::from_min_size(body.min, egui::vec2(nav_w, body.height()));
    painter.rect_filled(nav_rect, egui::CornerRadius::same(2), nav);

    // 侧栏选中块
    let nav_item = egui::Rect::from_min_size(
        egui::pos2(nav_rect.min.x + 1.5, nav_rect.min.y + 3.0),
        egui::vec2(nav_w - 3.0, 5.0),
    );
    painter.rect_filled(nav_item, egui::CornerRadius::same(1), accent);

    let content = egui::Rect::from_min_max(
        egui::pos2(nav_rect.max.x + 3.0, body.min.y),
        body.max,
    );
    // 主按钮
    let btn = egui::Rect::from_min_size(
        egui::pos2(content.min.x, content.min.y + 2.0),
        egui::vec2((content.width() * 0.55).max(10.0), 6.0),
    );
    painter.rect_filled(btn, egui::CornerRadius::same(2), accent);
    // 次按钮
    let btn2 = egui::Rect::from_min_size(
        egui::pos2(btn.max.x + 2.0, btn.min.y),
        egui::vec2((content.width() * 0.28).max(6.0), 6.0),
    );
    if btn2.max.x <= content.max.x {
        painter.rect_filled(btn2, egui::CornerRadius::same(2), muted);
    }
    // 文本行
    let line_w = content.width() * 0.75;
    for i in 0..2 {
        let y = btn.max.y + 4.0 + i as f32 * 5.0;
        if y + 2.0 > content.max.y {
            break;
        }
        let line = egui::Rect::from_min_size(
            egui::pos2(content.min.x, y),
            egui::vec2(line_w * (1.0 - i as f32 * 0.2), 2.5),
        );
        painter.rect_filled(line, egui::CornerRadius::same(1), muted);
    }
}

fn equal_primary(ui: &mut egui::Ui, label: &str, palette: Palette) -> bool {
    // 与次要按钮同色系，仅字重略重，避免深浅主题下黑白块过抢
    paint_action_btn(
        ui,
        label,
        palette.secondary_fill(),
        palette.secondary_hover(),
        palette.secondary_active(),
        palette.text,
        true,
    )
}

fn equal_secondary(ui: &mut egui::Ui, label: &str, palette: Palette) -> bool {
    paint_action_btn(
        ui,
        label,
        palette.secondary_fill(),
        palette.secondary_hover(),
        palette.secondary_active(),
        palette.text,
        false,
    )
}

fn paint_action_btn(
    ui: &mut egui::Ui,
    label: &str,
    fill: egui::Color32,
    hover: egui::Color32,
    active: egui::Color32,
    fg: egui::Color32,
    heavy: bool,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(BTN_W, BTN_H), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let bg = if resp.is_pointer_button_down_on() {
        active
    } else if resp.hovered() {
        hover
    } else {
        fill
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(10), bg);
    let font = if heavy {
        bold(14.5)
    } else {
        egui::FontId::proportional(14.5)
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        fg,
    );
    resp.clicked()
}

fn title_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    glyph: &str,
    id: egui::Id,
    palette: Palette,
) -> bool {
    let resp = ui
        .interact(rect, id, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let bg = if resp.is_pointer_button_down_on() {
        if palette.dark {
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 35)
        } else {
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 40)
        }
    } else if resp.hovered() {
        if palette.dark {
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18)
        } else {
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 18)
        }
    } else {
        egui::Color32::TRANSPARENT
    };
    if bg != egui::Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(7), bg);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(16.0),
        palette.muted,
    );
    resp.clicked()
}

fn load_tray_icon() -> Option<Icon> {
    let img = image::load_from_memory(TRAY_PNG).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).ok()
}

fn round_window(hwnd: HWND) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE,
    };
    let round: i32 = 2;
    let no_border: u32 = 0xFFFF_FFFE;
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

fn window_hwnd(cc: &eframe::CreationContext<'_>) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(w) => Some(HWND(w.hwnd.get() as *mut core::ffi::c_void)),
        _ => None,
    }
}
