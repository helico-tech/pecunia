//! Design tokens and fonts. Colours are our own; fonts come from the operating system at runtime,
//! with egui's built-in fonts as the fallback. The browser has no system fonts to read, so the web
//! build bundles Inter (OFL, subset to Latin, punctuation, arrows and key symbols; `assets/fonts/`).

use std::sync::Arc;

use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, Visuals};
use pecunia_engine::model::{Group, TxnStatus};

pub const UI_BOLD: &str = "ui-bold";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub dark: bool,
    /// Main content background.
    pub bg: Color32,
    /// Sidebar and board-column background.
    pub sidebar: Color32,
    /// Cards, inputs, popups.
    pub surface: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub accent_soft: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub danger: Color32,
    pub danger_soft: Color32,
    /// Money in, things done.
    pub positive: Color32,
    /// Needs a look: waiting in Review.
    pub warning: Color32,
    pub warning_soft: Color32,
    pub shadow: Color32,
}

impl Tokens {
    pub fn light() -> Tokens {
        Tokens {
            dark: false,
            bg: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            sidebar: Color32::from_rgb(0xF6, 0xF6, 0xF8),
            surface: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            border: Color32::from_rgb(0xE6, 0xE6, 0xEA),
            border_strong: Color32::from_rgb(0xD2, 0xD2, 0xD8),
            text: Color32::from_rgb(0x1E, 0x1F, 0x24),
            text_dim: Color32::from_rgb(0x5E, 0x61, 0x6B),
            text_faint: Color32::from_rgb(0x8E, 0x91, 0x9B),
            accent: Color32::from_rgb(0x0F, 0x76, 0x6E),
            accent_text: Color32::WHITE,
            accent_soft: Color32::from_rgb(0xE2, 0xF3, 0xF1),
            hover: Color32::from_rgb(0xF0, 0xF0, 0xF3),
            selected: Color32::from_rgb(0xE2, 0xF3, 0xF1),
            danger: Color32::from_rgb(0xD1, 0x3B, 0x3B),
            danger_soft: Color32::from_rgb(0xFC, 0xEB, 0xEB),
            positive: Color32::from_rgb(0x15, 0x8A, 0x5B),
            warning: Color32::from_rgb(0xC9, 0x8A, 0x04),
            warning_soft: Color32::from_rgb(0xFD, 0xF4, 0xDE),
            shadow: Color32::from_black_alpha(36),
        }
    }

    pub fn dark() -> Tokens {
        Tokens {
            dark: true,
            bg: Color32::from_rgb(0x19, 0x1A, 0x1E),
            sidebar: Color32::from_rgb(0x14, 0x15, 0x18),
            surface: Color32::from_rgb(0x23, 0x24, 0x29),
            border: Color32::from_rgb(0x2E, 0x30, 0x36),
            border_strong: Color32::from_rgb(0x3D, 0x40, 0x48),
            text: Color32::from_rgb(0xE8, 0xE9, 0xED),
            text_dim: Color32::from_rgb(0xA4, 0xA7, 0xB0),
            text_faint: Color32::from_rgb(0x72, 0x75, 0x7F),
            accent: Color32::from_rgb(0x2D, 0xB5, 0xA6),
            accent_text: Color32::WHITE,
            accent_soft: Color32::from_rgb(0x17, 0x36, 0x33),
            hover: Color32::from_rgb(0x24, 0x25, 0x2A),
            selected: Color32::from_rgb(0x1A, 0x3A, 0x37),
            danger: Color32::from_rgb(0xEF, 0x6B, 0x6B),
            danger_soft: Color32::from_rgb(0x3E, 0x23, 0x25),
            positive: Color32::from_rgb(0x4C, 0xC9, 0x93),
            warning: Color32::from_rgb(0xF0, 0xC2, 0x4A),
            warning_soft: Color32::from_rgb(0x3D, 0x33, 0x1C),
            shadow: Color32::from_black_alpha(110),
        }
    }

    pub fn get(dark: bool) -> Tokens {
        if dark { Tokens::dark() } else { Tokens::light() }
    }

    /// The colour of an account group: money you have, owe, spend, earn.
    pub fn group(&self, g: Group) -> Color32 {
        match (g, self.dark) {
            (Group::Have, false) => Color32::from_rgb(0x1E, 0x9E, 0x6A),
            (Group::Have, true) => Color32::from_rgb(0x4C, 0xC9, 0x93),
            (Group::Owe, false) => Color32::from_rgb(0xD2, 0x6A, 0x12),
            (Group::Owe, true) => Color32::from_rgb(0xF0, 0x9A, 0x4A),
            (Group::Goes, false) => Color32::from_rgb(0x2F, 0x6B, 0xE0),
            (Group::Goes, true) => Color32::from_rgb(0x6E, 0x9C, 0xF5),
            (Group::Comes, false) => Color32::from_rgb(0x7C, 0x4D, 0xDB),
            (Group::Comes, true) => Color32::from_rgb(0xA6, 0x86, 0xF5),
            (Group::Equity, _) => self.text_faint,
        }
    }

    /// The colour of a transaction's state.
    pub fn status(&self, s: TxnStatus) -> Color32 {
        match s {
            TxnStatus::Auto => self.text_faint,
            TxnStatus::ToReview => self.warning,
            TxnStatus::Reviewed => self.positive,
        }
    }

    /// A stable colour per label or person name, from our own palette.
    pub fn tag(&self, name: &str) -> Color32 {
        const LIGHT: [(u8, u8, u8); 8] = [
            (0x2F, 0x6B, 0xE0),
            (0x1E, 0x9E, 0x6A),
            (0xD2, 0x8A, 0x12),
            (0x7C, 0x4D, 0xDB),
            (0xC2, 0x3F, 0x8C),
            (0x0E, 0x8F, 0xA8),
            (0xB5, 0x5D, 0x1E),
            (0x5A, 0x6B, 0x7F),
        ];
        let h = name.to_lowercase().bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
        let (r, g, b) = LIGHT.get(h as usize % LIGHT.len()).copied().unwrap_or((0x5A, 0x6B, 0x7F));
        if self.dark {
            // Lighter for dark backgrounds.
            let up = |c: u8| c.saturating_add(((255 - c as u16) * 2 / 5) as u8);
            Color32::from_rgb(up(r), up(g), up(b))
        } else {
            Color32::from_rgb(r, g, b)
        }
    }
}

pub fn body(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(UI_BOLD.into()))
}

/// Amounts: the body font (right-aligned columns keep them readable).
pub fn mono_digits(size: f32) -> FontId {
    body(size)
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

fn load(paths: &[(&str, u32)]) -> Option<FontData> {
    paths.iter().find_map(|(p, index)| {
        std::fs::read(p).ok().map(|bytes| {
            let mut fd = FontData::from_owned(bytes);
            fd.index = *index;
            fd
        })
    })
}

#[cfg(target_arch = "wasm32")]
fn ui_fonts() -> (Option<FontData>, Option<FontData>) {
    (
        Some(FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf"))),
        Some(FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.ttf"))),
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn ui_fonts() -> (Option<FontData>, Option<FontData>) {
    let regular = load(&[
        ("/System/Library/Fonts/SFNS.ttf", 0),
        ("/System/Library/Fonts/HelveticaNeue.ttc", 0),
        ("C:\\Windows\\Fonts\\segoeui.ttf", 0),
        ("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf", 0),
        ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 0),
    ]);
    let strong = load(&[
        ("/System/Library/Fonts/HelveticaNeue.ttc", 10),
        ("/System/Library/Fonts/HelveticaNeue.ttc", 1),
        ("C:\\Windows\\Fonts\\seguisb.ttf", 0),
        ("/usr/share/fonts/truetype/noto/NotoSans-SemiBold.ttf", 0),
        ("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 0),
    ]);
    (regular, strong)
}

/// System UI fonts first (Inter on the web), egui's built-in fonts after them for anything missing.
pub fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let base_prop = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let base_mono = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let (regular, strong) = ui_fonts();
    let monospace =
        load(&[("/System/Library/Fonts/SFNSMono.ttf", 0), ("/System/Library/Fonts/Menlo.ttc", 0), ("C:\\Windows\\Fonts\\consola.ttf", 0)]);
    let mut prop = Vec::new();
    if let Some(fd) = regular {
        fonts.font_data.insert("sys-ui".into(), Arc::new(fd));
        prop.push("sys-ui".to_string());
    }
    let mut bold_chain = Vec::new();
    if let Some(fd) = strong {
        fonts.font_data.insert("sys-ui-bold".into(), Arc::new(fd));
        bold_chain.push("sys-ui-bold".to_string());
    }
    let mut mono_chain = Vec::new();
    if let Some(fd) = monospace {
        fonts.font_data.insert("sys-mono".into(), Arc::new(fd));
        mono_chain.push("sys-mono".to_string());
    }
    prop.extend(base_prop.iter().cloned());
    bold_chain.extend(prop.iter().cloned());
    mono_chain.extend(base_mono);
    fonts.families.insert(FontFamily::Proportional, prop);
    fonts.families.insert(FontFamily::Name(UI_BOLD.into()), bold_chain);
    fonts.families.insert(FontFamily::Monospace, mono_chain);
    fonts
}

/// Installs both palettes and the spacing; `dark` picks the one in use.
pub fn apply(ctx: &egui::Context, dark: bool) {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let t = Tokens::get(theme == egui::Theme::Dark);
        let mut v = if t.dark { Visuals::dark() } else { Visuals::light() };
        v.panel_fill = t.bg;
        v.window_fill = t.surface;
        v.window_stroke = Stroke::new(1.0, t.border);
        v.extreme_bg_color = t.surface;
        v.faint_bg_color = t.sidebar;
        v.selection.bg_fill = if t.dark { Color32::from_rgb(0x1F, 0x5C, 0x55) } else { Color32::from_rgb(0xC4, 0xE8, 0xE3) };
        v.selection.stroke = Stroke::new(1.0, t.accent);
        v.hyperlink_color = t.accent;
        v.override_text_color = None;
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
        v.widgets.inactive.bg_fill = t.surface;
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.border_strong);
        v.widgets.hovered.weak_bg_fill = t.hover;
        v.widgets.hovered.bg_fill = t.hover;
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, t.text);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.border_strong);
        v.widgets.active.weak_bg_fill = t.selected;
        v.widgets.active.bg_fill = t.selected;
        v.widgets.active.fg_stroke = Stroke::new(1.0, t.text);
        v.widgets.open.weak_bg_fill = t.hover;
        v.window_corner_radius = egui::CornerRadius::same(12);
        v.menu_corner_radius = egui::CornerRadius::same(8);
        v.window_shadow = egui::epaint::Shadow { offset: [0, 8], blur: 28, spread: 0, color: t.shadow };
        v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 16, spread: 0, color: t.shadow };
        v.text_cursor.stroke = Stroke::new(1.5, t.accent);
        ctx.set_visuals_of(theme, v);
    }
    ctx.options_mut(|o| o.fallback_theme = egui::Theme::Light);
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
        s.spacing.interact_size.y = 26.0;
        s.spacing.menu_margin = egui::Margin::same(6);
        s.text_styles.insert(egui::TextStyle::Body, body(13.0));
        s.text_styles.insert(egui::TextStyle::Button, body(13.0));
        s.text_styles.insert(egui::TextStyle::Small, body(11.0));
        s.text_styles.insert(egui::TextStyle::Heading, bold(20.0));
        s.text_styles.insert(egui::TextStyle::Monospace, mono(12.0));
        s.interaction.selectable_labels = false;
    });
}
