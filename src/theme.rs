use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle, Visuals,
};

// Design tokens. Every screen draws from these and nothing else: two surfaces,
// three text levels, one radius, a 4 px spacing scale, two control sizes.

/// The window, behind the content.
pub const BG: Color32 = Color32::from_rgb(0x0d, 0x0f, 0x13);
/// Header, sidebar, side panels and grouped settings.
pub const SURFACE: Color32 = Color32::from_rgb(0x14, 0x17, 0x1d);
/// Fills for controls and for rows under the pointer or selected.
pub const HOVER: Color32 = Color32::from_rgb(0x1f, 0x24, 0x2d);
pub const PRESSED: Color32 = Color32::from_rgb(0x2a, 0x30, 0x3b);
pub const SELECTED: Color32 = Color32::from_rgb(0x1b, 0x26, 0x3a);
pub const BORDER: Color32 = Color32::from_rgb(0x25, 0x2a, 0x33);
/// Links in the access graph.
pub const EDGE: Color32 = Color32::from_rgb(0x3a, 0x42, 0x50);
pub const TEXT: Color32 = Color32::from_rgb(0xe7, 0xe9, 0xee);
pub const MUTED: Color32 = Color32::from_rgb(0x8b, 0x93, 0xa1);
pub const FAINT: Color32 = Color32::from_rgb(0x56, 0x5d, 0x6a);
pub const ACCENT: Color32 = Color32::from_rgb(0x5b, 0x9b, 0xff);
pub const GREEN: Color32 = Color32::from_rgb(0x3d, 0xd6, 0x8c);
pub const AMBER: Color32 = Color32::from_rgb(0xf5, 0xa5, 0x24);
pub const RED: Color32 = Color32::from_rgb(0xf1, 0x60, 0x63);
pub const PURPLE: Color32 = Color32::from_rgb(0xb3, 0x94, 0xff);

/// The one corner radius, for buttons, chips, rows, cards and popups.
pub const RADIUS: f32 = 8.0;

/// Spacing scale.
pub mod sp {
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
    pub const L: f32 = 16.0;
    pub const XL: f32 = 24.0;
}

/// Type scale.
pub mod ty {
    pub const SMALL: f32 = 12.0;
    pub const BUTTON: f32 = 13.0;
    pub const BODY: f32 = 14.0;
    pub const HEADING: f32 = 20.0;
}

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
    pub const UPLOAD: &str = "\u{f093}";
    pub const ARROW_RIGHT: &str = "\u{f061}";
    pub const ARROW_LEFT: &str = "\u{f060}";
    pub const ARROWS_H: &str = "\u{f07e}";
}

/// How far a glyph's visual weight sits from the middle of its ink box, as a
/// share of the box, measured from the font. A paper plane or a tray with an
/// arrow looks off center even when its box is centered, so it is moved back by
/// half of this.
fn weight_offset(glyph: &str) -> egui::Vec2 {
    let (x, y) = match glyph {
        icon::SEND => (0.094, 0.016),
        icon::DOWNLOAD => (-0.008, 0.192),
        icon::UPLOAD => (-0.008, 0.140),
        icon::GRAPH => (0.080, 0.0),
        icon::WARN => (0.0, 0.123),
        icon::INBOX => (0.0, 0.075),
        icon::USER => (0.0, 0.091),
        icon::TAG => (-0.038, -0.038),
        icon::POWER => (0.0, 0.039),
        icon::ARROW_RIGHT => (0.095, 0.0),
        icon::ARROW_LEFT => (-0.095, 0.0),
        icon::COPY => (-0.024, 0.020),
        _ => (0.0, 0.0),
    };
    egui::vec2(x, y)
}

/// Paints an icon so its ink fills a `size` square, centered on `center` by eye.
/// Every glyph gets the same box, so a wide eye and a narrow bolt read as the
/// same size, and lopsided glyphs are nudged toward their visual middle.
pub fn paint_glyph(
    painter: &egui::Painter,
    center: egui::Pos2,
    glyph: &str,
    size: f32,
    color: Color32,
) {
    let ink = ink_rect(painter, glyph, size);
    let scale = if ink.is_positive() {
        size / ink.width().max(ink.height())
    } else {
        1.0
    };
    let font = size * scale;
    let nudge = -weight_offset(glyph) * 0.5 * size;
    paint_icon(painter, center + nudge, glyph, font, color);
}

fn ink_rect(painter: &egui::Painter, glyph: &str, size: f32) -> egui::Rect {
    let g = painter.layout_no_wrap(glyph.to_owned(), FontId::proportional(size), TEXT);
    let mut ink = egui::Rect::NOTHING;
    for row in &g.rows {
        for gl in &row.row.glyphs {
            if !gl.uv_rect.is_nothing() {
                let min = row.pos + gl.pos.to_vec2() + gl.uv_rect.offset;
                ink = ink.union(egui::Rect::from_min_size(min, gl.uv_rect.size));
            }
        }
    }
    ink
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
    let ring = if b.online { GREEN } else { EDGE };
    let fill = if b.me { ME_FILL } else { HOVER };
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

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    // The faces ship inside the app, subset to the scripts and icons it uses,
    // so it looks the same on systems without Noto or a Nerd Font.
    let mut add = |name: &str, bytes: &'static [u8], family: FontFamily, first: bool| {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
        let list = fonts.families.entry(family).or_default();
        if first {
            list.insert(0, name.to_owned())
        } else {
            list.push(name.to_owned())
        }
    };
    add(
        "ui",
        include_bytes!("../assets/fonts/NotoSans-Regular.ttf"),
        FontFamily::Proportional,
        true,
    );
    add(
        "mono",
        include_bytes!("../assets/fonts/JetBrainsMono-Medium.ttf"),
        FontFamily::Monospace,
        true,
    );
    // UI icons from the Nerd Font, as a fallback behind the UI face.
    add(
        "icons",
        include_bytes!("../assets/fonts/ui-icons.ttf"),
        FontFamily::Proportional,
        false,
    );
    add(
        "ui-strong",
        include_bytes!("../assets/fonts/NotoSans-Bold.ttf"),
        FontFamily::Name("strong".into()),
        true,
    );
    // OS logos ship with the app, right behind the UI face, so they show
    // without a Nerd Font and include logos newer than the system's.
    fonts.font_data.insert(
        "os-icons".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/os-icons.ttf"
        ))),
    );
    let list = fonts.families.entry(FontFamily::Proportional).or_default();
    list.insert(list.len().min(1), "os-icons".to_owned());
    fonts
        .families
        .entry(FontFamily::Name("strong".into()))
        .or_default()
        .extend(["os-icons".to_owned(), "icons".to_owned()]);
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(ty::SMALL)),
            (TextStyle::Body, FontId::proportional(ty::BODY)),
            (TextStyle::Button, FontId::proportional(ty::BUTTON)),
            (
                TextStyle::Heading,
                FontId::new(ty::HEADING, FontFamily::Name("strong".into())),
            ),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(sp::S, sp::S);
        style.spacing.button_padding = egui::vec2(sp::M, sp::S);
        style.spacing.interact_size.y = 32.0;
        style.spacing.menu_margin = Margin::same(sp::S as i8);
        style.interaction.selectable_labels = false;

        let mut v = Visuals::dark();
        v.panel_fill = SURFACE;
        v.window_fill = SURFACE;
        v.extreme_bg_color = SURFACE;
        v.faint_bg_color = SURFACE;
        v.override_text_color = Some(TEXT);
        v.hyperlink_color = ACCENT;
        v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.window_stroke = Stroke::new(1.0, BORDER);
        let r = CornerRadius::same(RADIUS as u8);
        v.window_corner_radius = r;
        v.menu_corner_radius = r;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.noninteractive.corner_radius = r;
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
        v.widgets.inactive.bg_fill = HOVER;
        v.widgets.inactive.weak_bg_fill = HOVER;
        // Only text fields and combo boxes still use egui's own frames.
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.inactive.corner_radius = r;
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.hovered.bg_fill = PRESSED;
        v.widgets.hovered.weak_bg_fill = PRESSED;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, EDGE);
        v.widgets.hovered.corner_radius = r;
        v.widgets.hovered.expansion = 0.0;
        v.widgets.active.bg_fill = PRESSED;
        v.widgets.active.weak_bg_fill = PRESSED;
        v.widgets.active.bg_stroke = Stroke::new(1.0, EDGE);
        v.widgets.active.corner_radius = r;
        v.widgets.active.expansion = 0.0;
        v.widgets.open.corner_radius = r;
        style.visuals = v;
    });

    // Native wheel feel: a notch is 100 px and egui eases to the summed target;
    // touchpads arrive as points and stay 1:1.
    ctx.options_mut(|o| o.input_options.line_scroll_speed = 100.0);
}
