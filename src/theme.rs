use std::process::Command;
use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle, Visuals,
};

pub const BG: Color32 = Color32::from_rgb(0x0d, 0x0f, 0x13);
pub const PANEL: Color32 = Color32::from_rgb(0x12, 0x15, 0x1a);
pub const CARD: Color32 = Color32::from_rgb(0x18, 0x1c, 0x23);
pub const CARD_HOVER: Color32 = Color32::from_rgb(0x1f, 0x24, 0x2d);
pub const CARD_SELECTED: Color32 = Color32::from_rgb(0x1c, 0x26, 0x3a);
pub const BORDER: Color32 = Color32::from_rgb(0x25, 0x2a, 0x33);
pub const EDGE: Color32 = Color32::from_rgb(0x3a, 0x42, 0x50);
pub const TEXT: Color32 = Color32::from_rgb(0xe7, 0xe9, 0xee);
pub const MUTED: Color32 = Color32::from_rgb(0x87, 0x90, 0x9e);
pub const FAINT: Color32 = Color32::from_rgb(0x52, 0x59, 0x66);
pub const ACCENT: Color32 = Color32::from_rgb(0x5b, 0x9b, 0xff);
pub const GREEN: Color32 = Color32::from_rgb(0x3d, 0xd6, 0x8c);
pub const AMBER: Color32 = Color32::from_rgb(0xf5, 0xa5, 0x24);
pub const RED: Color32 = Color32::from_rgb(0xf1, 0x60, 0x63);
pub const PURPLE: Color32 = Color32::from_rgb(0xb3, 0x94, 0xff);

/// Font Awesome codepoints, present in every Nerd Font.
pub mod icon {
    pub const POWER: &str = "\u{f011}";
    pub const REFRESH: &str = "\u{f021}";
    pub const COPY: &str = "\u{f0c5}";
    pub const SEND: &str = "\u{f1d8}";
    pub const DOWNLOAD: &str = "\u{f019}";
    pub const SEARCH: &str = "\u{f422}";
    pub const TAG: &str = "\u{f02b}";
    pub const USER: &str = "\u{f007}";
    pub const GLOBE: &str = "\u{f0ac}";
    pub const GRAPH: &str = "\u{f1e0}";
    pub const LIST: &str = "\u{f03a}";
    pub const BOLT: &str = "\u{f0e7}";
    pub const LINUX: &str = "\u{f17c}";
    pub const ANDROID: &str = "\u{f17b}";
    pub const WINDOWS: &str = "\u{f17a}";
    pub const APPLE: &str = "\u{f179}";
    pub const SERVER: &str = "\u{f233}";
    pub const WARN: &str = "\u{f071}";
    pub const INBOX: &str = "\u{f01c}";
    pub const CLOSE: &str = "\u{f00d}";
    pub const TRASH: &str = "\u{f1f8}";
    pub const BAN: &str = "\u{f05e}";
    pub const CHECK: &str = "\u{f00c}";
    pub const CLOCK: &str = "\u{f017}";
    pub const EYE: &str = "\u{f06e}";
    pub const KEY: &str = "\u{f084}";
    pub const LOCK: &str = "\u{f023}";
    pub const PLUG: &str = "\u{f1e6}";
    pub const EYE_SLASH: &str = "\u{f070}";
    pub const GEAR: &str = "\u{f013}";
    pub const CHEVRON_DOWN: &str = "\u{f078}";
    pub const CHEVRON_RIGHT: &str = "\u{f054}";
    pub const HELP: &str = "\u{f059}";
    pub const EXTERNAL: &str = "\u{f08e}";
}

pub fn os_icon(os: &str) -> &'static str {
    match os {
        "linux" => icon::LINUX,
        "android" => icon::ANDROID,
        "windows" => icon::WINDOWS,
        "macOS" | "iOS" | "tvOS" => icon::APPLE,
        _ => icon::SERVER,
    }
}

/// Paints one icon glyph with its ink centered on `center`. Text layout centers the
/// advance box, and Nerd Font glyphs sit off center inside theirs.
pub fn paint_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    glyph: &str,
    size: f32,
    color: Color32,
) {
    let g = painter.layout_no_wrap(glyph.to_owned(), FontId::proportional(size), color);
    let mut ink = egui::Rect::NOTHING;
    for row in &g.rows {
        for gl in &row.row.glyphs {
            if !gl.uv_rect.is_nothing() {
                let min = row.pos + gl.pos.to_vec2() + gl.uv_rect.offset;
                ink = ink.union(egui::Rect::from_min_size(min, gl.uv_rect.size));
            }
        }
    }
    let mid = if ink.is_positive() {
        ink.center()
    } else {
        g.rect.center()
    };
    painter.galley(center - mid.to_vec2(), g, color);
}

/// How a device badge looks: its status sets the ring, the rest sets fill and shape.
#[derive(Clone, Copy)]
pub struct Badge {
    pub online: bool,
    pub me: bool,
    pub tagged: bool,
}

impl Badge {
    pub fn of(n: &crate::ts::Node) -> Self {
        Self {
            // This device is always up from its own point of view.
            online: n.online || n.is_self,
            me: n.is_self,
            tagged: !n.tags.is_empty(),
        }
    }
}

pub const RING_OFF: Color32 = Color32::from_rgb(0x3a, 0x41, 0x4d);
pub const ME_FILL: Color32 = Color32::from_rgb(0x16, 0x2a, 0x4d);

/// A device everywhere in the app: its OS glyph inside a ring that is green while
/// the device is online and grey when it is not. Tagged devices are squares.
pub fn paint_badge(
    painter: &egui::Painter,
    center: egui::Pos2,
    r: f32,
    glyph: &str,
    b: Badge,
    alpha: f32,
) {
    let ring = if b.online { GREEN } else { RING_OFF };
    let fill = if b.me { ME_FILL } else { CARD_HOVER };
    let width = (r * 0.09).clamp(1.5, 2.0);
    let shape = |rect_r: f32, f: Color32, s: Stroke| {
        if b.tagged {
            let rr = egui::Rect::from_center_size(center, egui::Vec2::splat(rect_r * 1.8));
            let corner = rect_r * 0.4;
            painter.rect_filled(rr, corner, f);
            painter.rect_stroke(rr, corner, s, egui::StrokeKind::Middle);
        } else {
            painter.circle_filled(center, rect_r, f);
            painter.circle_stroke(center, rect_r, s);
        }
    };
    shape(
        r,
        fill.gamma_multiply(alpha),
        Stroke::new(width, ring.gamma_multiply(alpha)),
    );
    let icon_col = if b.online { TEXT } else { MUTED };
    paint_icon(
        painter,
        center,
        glyph,
        r * 0.9,
        icon_col.gamma_multiply(alpha),
    );
}

/// Tailscale's mark, a 3x3 grid of dots with a solid T, marked as unofficial by
/// lighting the top right dot in this app's accent. Same drawing as assets/mark.svg.
pub fn paint_mark(painter: &egui::Painter, rect: egui::Rect) {
    let step = rect.width() / 3.0;
    let r = step * 0.37;
    for row in 0..3 {
        for col in 0..3 {
            let c = rect.min + egui::vec2((col as f32 + 0.5) * step, (row as f32 + 0.5) * step);
            let fill = if row == 0 && col == 2 {
                ACCENT
            } else if row == 1 || (row == 2 && col == 1) {
                TEXT
            } else {
                TEXT.gamma_multiply(0.22)
            };
            painter.circle_filled(c, r, fill);
        }
    }
}

fn font_file(pattern: &str) -> Option<Vec<u8>> {
    let out = Command::new("fc-match")
        .args(["-f", "%{file}", pattern])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&out.stdout).to_string();
    std::fs::read(path.trim()).ok()
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, pattern: &str, family: FontFamily, first: bool| {
        if let Some(bytes) = font_file(pattern) {
            fonts
                .font_data
                .insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
            let list = fonts.families.entry(family).or_default();
            if first {
                list.insert(0, name.to_owned())
            } else {
                list.push(name.to_owned())
            }
        }
    };
    add("ui", "Noto Sans:weight=80", FontFamily::Proportional, true);
    add(
        "mono",
        "JetBrainsMono Nerd Font:style=Medium",
        FontFamily::Monospace,
        true,
    );
    // Icons come from the Nerd Font, as a fallback behind the UI face.
    add(
        "icons",
        "JetBrainsMono Nerd Font",
        FontFamily::Proportional,
        false,
    );
    add(
        "ui-strong",
        "Noto Sans:weight=200",
        FontFamily::Name("strong".into()),
        true,
    );
    fonts
        .families
        .entry(FontFamily::Name("strong".into()))
        .or_default()
        .extend(["icons".to_owned()]);
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(11.5)),
            (TextStyle::Body, FontId::proportional(14.0)),
            (TextStyle::Button, FontId::proportional(13.5)),
            (
                TextStyle::Heading,
                FontId::new(20.0, FontFamily::Name("strong".into())),
            ),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 30.0;
        style.spacing.menu_margin = Margin::same(8);
        style.interaction.selectable_labels = false;

        let mut v = Visuals::dark();
        v.panel_fill = PANEL;
        v.window_fill = CARD;
        v.extreme_bg_color = BG;
        v.faint_bg_color = CARD;
        v.override_text_color = Some(TEXT);
        v.hyperlink_color = ACCENT;
        v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(10);
        let r = CornerRadius::same(8);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.noninteractive.corner_radius = r;
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
        v.widgets.inactive.bg_fill = CARD_HOVER;
        v.widgets.inactive.weak_bg_fill = CARD_HOVER;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.inactive.corner_radius = r;
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.hovered.bg_fill = Color32::from_rgb(0x2a, 0x30, 0x3b);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x2a, 0x30, 0x3b);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x3a, 0x42, 0x50));
        v.widgets.hovered.corner_radius = r;
        v.widgets.hovered.expansion = 0.0;
        v.widgets.active.bg_fill = Color32::from_rgb(0x33, 0x3a, 0x47);
        v.widgets.active.weak_bg_fill = Color32::from_rgb(0x33, 0x3a, 0x47);
        v.widgets.active.corner_radius = r;
        v.widgets.active.expansion = 0.0;
        v.widgets.open.corner_radius = r;
        style.visuals = v;
    });

    // Native wheel feel: a notch is 100 px and egui eases to the summed target;
    // touchpads arrive as points and stay 1:1.
    ctx.options_mut(|o| o.input_options.line_scroll_speed = 100.0);
}
