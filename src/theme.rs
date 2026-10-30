//! 浅色 / 深色调色（透明底 + Acrylic）。

use eframe::egui;

use crate::fonts;

#[derive(Clone, Copy)]
pub struct Palette {
    pub accent: egui::Color32,
    pub accent_hover: egui::Color32,
    pub accent_active: egui::Color32,
    pub text: egui::Color32,
    pub muted: egui::Color32,
    pub highlight: egui::Color32,
    pub dark: bool,
}

impl Palette {
    pub fn for_dark(dark: bool) -> Self {
        if dark {
            Self {
                accent: egui::Color32::from_rgb(235, 235, 240),
                accent_hover: egui::Color32::from_rgb(210, 210, 218),
                accent_active: egui::Color32::from_rgb(255, 255, 255),
                text: egui::Color32::from_rgb(240, 240, 245),
                muted: egui::Color32::from_rgb(160, 160, 170),
                highlight: egui::Color32::from_rgba_unmultiplied(255, 170, 60, 55),
                dark: true,
            }
        } else {
            Self {
                accent: egui::Color32::from_rgb(29, 29, 31),
                accent_hover: egui::Color32::from_rgb(58, 58, 61),
                accent_active: egui::Color32::from_rgb(0, 0, 0),
                text: egui::Color32::from_rgb(29, 29, 31),
                muted: egui::Color32::from_rgb(110, 110, 115),
                highlight: egui::Color32::from_rgba_unmultiplied(255, 150, 40, 40),
                dark: false,
            }
        }
    }

    pub fn nav_bg(self) -> egui::Color32 {
        if self.dark {
            // 深色：叠一层浅白，侧栏比主区略亮
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18)
        } else {
            // 浅色：明显深一点的灰底，安装包/亚克力下也能看出侧栏
            egui::Color32::from_rgb(220, 222, 228)
        }
    }

    pub fn nav_active(self) -> egui::Color32 {
        if self.dark {
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 28)
        } else {
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 22)
        }
    }

    pub fn fill(self, glass: bool) -> egui::Color32 {
        if glass {
            if self.dark {
                egui::Color32::from_rgba_unmultiplied(28, 28, 32, 150)
            } else {
                egui::Color32::from_rgba_unmultiplied(252, 252, 254, 148)
            }
        } else if self.dark {
            egui::Color32::from_rgb(32, 32, 36)
        } else {
            egui::Color32::from_rgb(248, 250, 252)
        }
    }

    /// 次要 / 统一按钮底
    pub fn secondary_fill(self) -> egui::Color32 {
        if self.dark {
            egui::Color32::from_rgb(58, 58, 64)
        } else {
            egui::Color32::from_rgb(232, 232, 236)
        }
    }

    pub fn secondary_hover(self) -> egui::Color32 {
        if self.dark {
            egui::Color32::from_rgb(72, 72, 80)
        } else {
            egui::Color32::from_rgb(220, 220, 226)
        }
    }

    pub fn secondary_active(self) -> egui::Color32 {
        if self.dark {
            egui::Color32::from_rgb(88, 88, 96)
        } else {
            egui::Color32::from_rgb(208, 208, 214)
        }
    }

    /// 选中块：比底略亮/略深，避免纯白/纯黑硬对比
    pub fn selected_fill(self) -> egui::Color32 {
        if self.dark {
            egui::Color32::from_rgb(78, 80, 90)
        } else {
            egui::Color32::from_rgb(214, 216, 222)
        }
    }

    pub fn selected_hover(self) -> egui::Color32 {
        if self.dark {
            egui::Color32::from_rgb(90, 92, 102)
        } else {
            egui::Color32::from_rgb(200, 202, 210)
        }
    }

    /// 选中描边：中性灰蓝，深浅主题都柔和
    pub fn selected_stroke(self) -> egui::Color32 {
        if self.dark {
            egui::Color32::from_rgb(150, 155, 170)
        } else {
            egui::Color32::from_rgb(100, 105, 120)
        }
    }

    /// TextEdit / 输入框底色
    pub fn input_bg(self) -> egui::Color32 {
        if self.dark {
            egui::Color32::from_rgb(40, 40, 46)
        } else {
            egui::Color32::from_rgb(255, 255, 255)
        }
    }

    pub fn input_stroke(self) -> egui::Color32 {
        if self.dark {
            egui::Color32::from_rgb(70, 70, 78)
        } else {
            egui::Color32::from_rgb(210, 212, 218)
        }
    }
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = egui::Color32::TRANSPARENT;
    visuals.window_fill = egui::Color32::TRANSPARENT;

    // 显式设定输入框底，避免主题切换后仍沿用深色 extreme_bg
    if dark {
        visuals.extreme_bg_color = egui::Color32::from_rgb(40, 40, 46);
        visuals.faint_bg_color = egui::Color32::from_rgb(48, 48, 54);
        visuals.text_cursor.stroke.color = egui::Color32::from_rgb(220, 220, 230);
    } else {
        visuals.extreme_bg_color = egui::Color32::from_rgb(255, 255, 255);
        visuals.faint_bg_color = egui::Color32::from_rgb(240, 241, 244);
        visuals.text_cursor.stroke.color = egui::Color32::from_rgb(40, 40, 45);
        // 浅色控件描边/底，防止某些 widget 仍偏黑
        for w in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            if w.bg_fill.r() < 40 && w.bg_fill.g() < 40 && w.bg_fill.b() < 40 {
                w.bg_fill = egui::Color32::from_rgb(245, 245, 248);
            }
            if w.weak_bg_fill.r() < 40 && w.weak_bg_fill.g() < 40 && w.weak_bg_fill.b() < 40 {
                w.weak_bg_fill = egui::Color32::from_rgb(236, 236, 240);
            }
        }
    }

    let theme = if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    let mut style = (*ctx.style_of(theme)).clone();
    style.visuals = visuals.clone();
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::proportional(15.0),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::proportional(14.5),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::proportional(13.0),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::proportional(20.0),
    );
    let _ = fonts::bold(14.0);
    ctx.set_style_of(theme, style);
    ctx.set_visuals(visuals);
    ctx.set_theme(if dark {
        egui::ThemePreference::Dark
    } else {
        egui::ThemePreference::Light
    });
}
