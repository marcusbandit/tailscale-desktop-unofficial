//! The controls every screen shares, drawn by hand so sizes, radius and icon
//! placement come from the tokens in `theme` instead of egui's text layout.

use eframe::egui::{
    self, Color32, CursorIcon, FontId, Galley, Painter, Pos2, Rect, Response, RichText, Sense,
    Stroke, Ui, Widget, vec2,
};
use std::sync::Arc;

use crate::theme::*;

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// The one main action on a screen or in a panel.
    Primary,
    /// Everything else that is clearly a button.
    Secondary,
    /// A button with no fill until the pointer is on it.
    Ghost,
    /// The fix in a warning.
    Warn,
}

/// Two control heights. Small sits inside rows and banners, medium everywhere else.
#[derive(Clone, Copy, PartialEq)]
pub enum Size {
    Sm,
    Md,
}

impl Size {
    pub fn height(self) -> f32 {
        match self {
            Size::Sm => 28.0,
            Size::Md => 32.0,
        }
    }

    /// The icon box that goes with the height.
    pub fn icon(self) -> f32 {
        match self {
            Size::Sm => 13.0,
            Size::Md => 15.0,
        }
    }

    fn pad(self) -> f32 {
        match self {
            Size::Sm => 10.0,
            Size::Md => sp::M,
        }
    }
}

/// A button: an optional icon, optional text, one of the two heights. With no
/// text it is a square with the icon in the middle.
pub struct Btn<'a> {
    text: &'a str,
    glyph: Option<&'a str>,
    kind: Kind,
    size: Size,
    color: Option<Color32>,
    on: bool,
}

impl<'a> Btn<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            glyph: None,
            kind: Kind::Secondary,
            size: Size::Md,
            color: None,
            on: false,
        }
    }

    /// A square icon button.
    pub fn icon(glyph: &'a str) -> Self {
        Self::new("").glyph(glyph).kind(Kind::Ghost)
    }

    pub fn glyph(mut self, glyph: &'a str) -> Self {
        self.glyph = Some(glyph);
        self
    }

    pub fn kind(mut self, kind: Kind) -> Self {
        self.kind = kind;
        self
    }

    pub fn primary(self) -> Self {
        self.kind(Kind::Primary)
    }

    pub fn small(mut self) -> Self {
        self.size = Size::Sm;
        self
    }

    /// Paints icon and text in this color instead of the kind's.
    pub fn color(mut self, c: Color32) -> Self {
        self.color = Some(c);
        self
    }

    /// Shown as switched on, for toggles such as privacy mode.
    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }
}

impl Widget for Btn<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let enabled = ui.is_enabled();
        let h = self.size.height();
        let ib = self.size.icon();
        let (bg_fill, fg) = match self.kind {
            Kind::Primary => (ACCENT, Color32::WHITE),
            Kind::Warn => (AMBER, Color32::BLACK),
            Kind::Secondary => (HOVER, TEXT),
            Kind::Ghost => (Color32::TRANSPARENT, MUTED),
        };
        let fg = self.color.unwrap_or(if self.on { ACCENT } else { fg });
        let galley = (!self.text.is_empty()).then(|| {
            ui.painter()
                .layout_no_wrap(self.text.to_owned(), FontId::proportional(ty::BUTTON), fg)
        });
        let w = match (&galley, self.glyph) {
            (None, _) => h,
            (Some(g), None) => g.size().x + self.size.pad() * 2.0,
            (Some(g), Some(_)) => g.size().x + ib + sp::S + self.size.pad() * 2.0 - 2.0,
        };
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, resp) = ui.allocate_exact_size(vec2(w, h), sense);
        if !ui.is_rect_visible(rect) {
            return resp;
        }
        let pressed = resp.is_pointer_button_down_on();
        let hovered = resp.hovered() && enabled;
        let fill = match self.kind {
            Kind::Primary | Kind::Warn => {
                if pressed {
                    bg_fill.gamma_multiply(0.8)
                } else if hovered {
                    bg_fill.gamma_multiply(0.92)
                } else {
                    bg_fill.gamma_multiply(0.85)
                }
            }
            Kind::Secondary => {
                if pressed || hovered {
                    PRESSED
                } else {
                    bg_fill
                }
            }
            Kind::Ghost => {
                if pressed {
                    PRESSED
                } else if hovered {
                    HOVER
                } else if self.on {
                    ACCENT.gamma_multiply(0.16)
                } else {
                    bg_fill
                }
            }
        };
        let alpha = if enabled { 1.0 } else { 0.4 };
        let p = ui.painter();
        p.rect_filled(rect, RADIUS, fill.gamma_multiply(alpha));
        let fg = if hovered && self.kind == Kind::Ghost && self.color.is_none() && !self.on {
            TEXT
        } else {
            fg
        };
        let fg = fg.gamma_multiply(alpha);
        match (galley, self.glyph) {
            (None, Some(g)) => paint_glyph(p, rect.center(), g, ib, fg),
            (Some(gal), glyph) => {
                let mut x = rect.left() + self.size.pad();
                if let Some(g) = glyph {
                    x -= 1.0;
                    paint_glyph(p, egui::pos2(x + ib / 2.0, rect.center().y), g, ib, fg);
                    x += ib + sp::S;
                }
                text_at_mid(p, egui::pos2(x, rect.center().y), gal, fg);
            }
            (None, None) => {}
        }
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        resp
    }
}

/// Places a one-line galley so the middle of its capitals sits on `left_mid.y`,
/// which is where an icon beside it is centered.
pub fn text_at_mid(p: &Painter, left_mid: Pos2, galley: Arc<Galley>, color: Color32) {
    let size = galley.rows.first().and_then(|r| {
        r.row
            .glyphs
            .first()
            .map(|g| (r.pos.y + g.pos.y, g.font_height))
    });
    let top = match size {
        Some((baseline, font_h)) => {
            // Noto Sans capitals are 0.714 em tall; the em is about 0.73 of the
            // line height egui reports.
            let cap = font_h * 0.73 * 0.714;
            left_mid.y - (baseline - cap / 2.0)
        }
        None => left_mid.y - galley.size().y / 2.0,
    };
    // The galley may carry its own color from layout; this one wins.
    p.add(
        egui::epaint::TextShape::new(egui::pos2(left_mid.x, top), galley, color)
            .with_override_text_color(color),
    );
}

/// Text placed with `text_at_mid`, for painters that draw their own rows.
pub fn paint_text(p: &Painter, left_mid: Pos2, text: &str, size: f32, color: Color32) -> Rect {
    let g = p.layout_no_wrap(text.to_owned(), FontId::proportional(size), color);
    let r = Rect::from_min_size(
        egui::pos2(left_mid.x, left_mid.y - g.size().y / 2.0),
        g.size(),
    );
    text_at_mid(p, left_mid, g, color);
    r
}

/// A link: colored text, an optional trailing icon, no box.
pub fn link(ui: &mut Ui, text: &str, glyph: Option<&str>, color: Color32) -> Response {
    let g = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(ty::BUTTON), color);
    let ib = 11.0;
    let w = g.size().x + glyph.map_or(0.0, |_| ib + 6.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, Size::Sm.height()), Sense::click());
    let col = if resp.hovered() {
        color
    } else {
        color.gamma_multiply(0.9)
    };
    let gw = g.size().x;
    text_at_mid(ui.painter(), rect.left_center(), g, col);
    if let Some(gl) = glyph {
        paint_glyph(
            ui.painter(),
            egui::pos2(rect.left() + gw + 6.0 + ib / 2.0, rect.center().y),
            gl,
            ib,
            col,
        );
    }
    if resp.hovered() {
        let y = rect.center().y + 8.0;
        ui.painter().hline(
            rect.left()..=rect.left() + gw,
            y,
            Stroke::new(1.0, col.gamma_multiply(0.6)),
        );
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp
}

/// A small colored label with an optional leading icon.
pub fn chip(ui: &mut Ui, glyph: Option<&str>, text: &str, color: Color32) -> Response {
    let g = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(ty::SMALL), color);
    let ib = 10.0;
    let h = 20.0;
    let lead = glyph.map_or(0.0, |_| ib + sp::XS + 1.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + lead + 14.0, h), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, RADIUS - 2.0, color.gamma_multiply(0.14));
    let mut x = rect.left() + 7.0;
    if let Some(gl) = glyph {
        paint_glyph(p, egui::pos2(x + ib / 2.0, rect.center().y), gl, ib, color);
        x += lead;
    }
    text_at_mid(p, egui::pos2(x, rect.center().y), g, color);
    resp
}

/// A page's title, with room on the right for its controls.
pub fn page_title(ui: &mut Ui, title: &str, sub: &str, right: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(32.0);
        ui.label(RichText::new(title).heading());
        if !sub.is_empty() {
            ui.add_space(sp::XS);
            ui.label(RichText::new(sub).size(ty::BODY).color(MUTED));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
    ui.add_space(sp::L);
}

/// A section's name, small and muted, over plain content.
pub fn section(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).size(ty::SMALL).color(MUTED).strong());
    ui.add_space(sp::XS);
}

/// A thin line across the available width, with space on both sides.
pub fn divider(ui: &mut Ui, space: f32) {
    ui.add_space(space);
    let r = ui.available_rect_before_wrap();
    ui.painter()
        .hline(r.left()..=r.right(), r.top(), Stroke::new(1.0, BORDER));
    ui.add_space(space + 1.0);
}

/// A centered glyph with its label to the right; used by navigation rows.
pub fn icon_label(
    p: &Painter,
    icon_c: Pos2,
    glyph: &str,
    ib: f32,
    text: &str,
    color: Color32,
    icon_color: Color32,
) {
    paint_glyph(p, icon_c, glyph, ib, icon_color);
    paint_text(
        p,
        icon_c + vec2(ib / 2.0 + 12.0, 0.0),
        text,
        ty::BODY,
        color,
    );
}

/// Text anchored on the right edge, vertically centered by its capitals.
pub fn paint_text_right(p: &Painter, right_mid: Pos2, text: &str, size: f32, color: Color32) {
    let g = p.layout_no_wrap(text.to_owned(), FontId::proportional(size), color);
    let x = right_mid.x - g.size().x;
    text_at_mid(p, egui::pos2(x, right_mid.y), g, color);
}

/// A centered column at most `max` wide, the frame for Taildrop and Settings.
pub fn column(ui: &mut Ui, max: f32, add: impl FnOnce(&mut Ui)) {
    let w = max.min(ui.available_width());
    let pad = ((ui.available_width() - w) / 2.0).max(0.0);
    ui.horizontal(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_width(w);
            add(ui);
        });
    });
}

/// How wide Taildrop and Settings are.
pub const COLUMN: f32 = 760.0;
