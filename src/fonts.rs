//! 使用 egui 自带默认字体；中文回退系统字体（不再使用 NotoSansSC）。

use std::sync::Arc;

use eframe::egui;

const BOLD: &str = "bold";

pub fn bold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(BOLD.into()))
}

pub fn configure(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let mut chain = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    if let Some((name, data)) = load_system_cjk() {
        fonts.font_data.insert(name.clone(), Arc::new(data));
        // CJK 放前，中文排版更稳
        chain.insert(0, name);
    }

    fonts
        .families
        .insert(egui::FontFamily::Proportional, chain.clone());
    fonts
        .families
        .insert(egui::FontFamily::Name(BOLD.into()), chain);
    ctx.set_fonts(fonts);
}

fn load_system_cjk() -> Option<(String, egui::FontData)> {
    let windir = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
    let fonts = std::path::PathBuf::from(windir).join("Fonts");
    for (name, index) in [
        ("msyh.ttc", 0u32),
        ("msyh.ttf", 0),
        ("simhei.ttf", 0),
        ("simsun.ttc", 0),
    ] {
        let path = fonts.join(name);
        if let Ok(bytes) = std::fs::read(&path) {
            let mut data = egui::FontData::from_owned(bytes);
            data.index = index;
            return Some(("cjk".to_owned(), data));
        }
    }
    None
}
