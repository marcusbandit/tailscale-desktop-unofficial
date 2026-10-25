//! A miniature of the Tailscale admin console that replays every click for making
//! the API credential, one screen at a time. The screens are drawn after the
//! console's own pages, in its own colors, so they read as the other window.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, pos2, vec2,
};

use crate::theme::{icon, *};

// The console's dark theme, from its saved pages.
const PAGE: Color32 = Color32::from_rgb(31, 30, 30);
const BAR: Color32 = Color32::from_rgb(18, 17, 17);
const LINE: Color32 = Color32::from_rgb(56, 54, 53);
const FIELD: Color32 = Color32::from_rgb(40, 39, 38);
const INK: Color32 = Color32::from_rgb(236, 234, 231);
const DIM: Color32 = Color32::from_rgb(158, 155, 151);
const BLUE: Color32 = Color32::from_rgb(75, 112, 204);
const LINK: Color32 = Color32::from_rgb(108, 148, 236);

const HEIGHT: f32 = 246.0;
const CHROME: f32 = 26.0;

// The pacing of one screen: a pause, then per click a glide, a press and a
// moment to see what changed, then a hold before the next screen.
const LEAD: f32 = 0.5;
const GLIDE: f32 = 0.8;
const PRESS: f32 = 0.2;
const AFTER: f32 = 0.5;
const HOLD: f32 = 1.6;
const CLICK: f32 = GLIDE + PRESS + AFTER;
/// How far the scopes page scrolls to bring Generate credential into view.
const SCROLL: f32 = 150.0;

struct Scene {
    caption: &'static str,
    path: &'static str,
    clicks: usize,
}

const SCENES: [Scene; 5] = [
    Scene {
        caption: "Press Credential.",
        path: "/admin/settings/trust-credentials",
        clicks: 1,
    },
    Scene {
        caption: "OAuth is already picked. Press Continue.",
        path: "/admin/settings/trust-credentials/add",
        clicks: 1,
    },
    Scene {
        caption: "Open the Custom scopes menu and pick All - Read.",
        path: "/admin/settings/trust-credentials/add",
        clicks: 2,
    },
    Scene {
        caption: "Scroll down and press Generate credential.",
        path: "/admin/settings/trust-credentials/add",
        clicks: 1,
    },
    Scene {
        caption: "Copy the client secret. Tailscale shows it only once.",
        path: "/admin/settings/trust-credentials/add",
        clicks: 1,
    },
];

/// Which screen is playing and when it started. `TSDU_SCENE=<n>` pins screen n,
/// counted from 1 like the written steps, at its last click, for captures.
/// `TSDU_SCENE=<n>:<click>` pins it at that click instead, also from 1.
pub struct Replay {
    scene: usize,
    t0: Option<f64>,
    /// The last frame drawn, so time spent away from the page or unfocused is
    /// skipped and the replay resumes where it was.
    last: f64,
    /// The click a pinned screen holds at, counted from 1.
    pinned: Option<usize>,
}

impl Default for Replay {
    fn default() -> Self {
        let pin = std::env::var("TSDU_SCENE").ok().and_then(|v| {
            let (scene, click) = v.split_once(':').unwrap_or((&v, ""));
            Some((scene.parse::<usize>().ok()?, click.parse::<usize>().ok()))
        });
        let scene = pin.map_or(1, |p| p.0).clamp(1, SCENES.len()) - 1;
        let clicks = SCENES[scene].clicks;
        Self {
            scene,
            t0: None,
            last: 0.0,
            pinned: pin.map(|p| p.1.unwrap_or(clicks).clamp(1, clicks)),
        }
    }
}

fn scene_len(clicks: usize) -> f32 {
    LEAD + clicks as f32 * CLICK + HOLD
}

/// Clicks that have landed by `t`; a click counts from the middle of its press.
fn landed(t: f32, clicks: usize) -> usize {
    (0..clicks)
        .filter(|&i| t >= LEAD + i as f32 * CLICK + GLIDE + PRESS * 0.5)
        .count()
}

fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x < 0.5 {
        4.0 * x * x * x
    } else {
        1.0 - (-2.0 * x + 2.0).powi(3) / 2.0
    }
}

pub fn show(ui: &mut Ui, r: &mut Replay) {
    let (now, focused) = ui.input(|i| (i.time, i.focused));
    let gap = if r.t0.is_some() { now - r.last } else { 0.0 };
    r.last = now;
    // Away from the page or out of focus, the replay waits instead of running on.
    let t0 = r.t0.get_or_insert(now);
    if gap > 0.25 || !focused {
        *t0 += gap;
    }
    let mut t = (now - *t0) as f32;
    if let Some(n) = r.pinned {
        t = LEAD + (n - 1) as f32 * CLICK + GLIDE + PRESS * 0.6;
    } else if t > scene_len(SCENES[r.scene].clicks) {
        r.scene = (r.scene + 1) % SCENES.len();
        r.t0 = Some(now);
        t = 0.0;
    }
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), Sense::hover());
    window(ui, rect, r.scene, t);
    // About 30 frames a second is plenty for a cursor, and nothing while unfocused.
    if r.pinned.is_none() && focused {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }

    ui.add_space(12.0);
    for (i, s) in SCENES.iter().enumerate() {
        let progress = if i == r.scene && r.pinned.is_none() {
            Some((t / scene_len(s.clicks)).min(1.0))
        } else {
            None
        };
        if step_row(ui, i, s.caption, i == r.scene, progress).clicked() {
            r.scene = i;
            r.t0 = Some(now);
            r.pinned = None;
        }
    }
}

/// One line of the written steps. The playing one is lit, with a ring around its
/// number that fills as the screen plays; any line can be picked to replay it.
fn step_row(ui: &mut Ui, i: usize, text: &str, on: bool, progress: Option<f32>) -> egui::Response {
    let w = ui.available_width();
    let wrap = w - 44.0;
    let color = if on { TEXT } else { MUTED };
    let galley = ui
        .painter()
        .layout(text.to_owned(), FontId::proportional(13.5), color, wrap);
    let h = galley.size().y.max(18.0) + 10.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let p = ui.painter();
    if on || resp.hovered() {
        p.rect_filled(
            rect,
            8.0,
            if on {
                CARD_HOVER
            } else {
                CARD_HOVER.gamma_multiply(0.5)
            },
        );
    }
    let c = pos2(rect.left() + 17.0, rect.top() + 14.0);
    p.circle_filled(
        c,
        10.0,
        if on {
            ACCENT.gamma_multiply(0.85)
        } else {
            CARD_HOVER
        },
    );
    p.text(
        c,
        Align2::CENTER_CENTER,
        (i + 1).to_string(),
        FontId::proportional(11.5),
        if on { Color32::WHITE } else { MUTED },
    );
    if let Some(f) = progress {
        arc(p, c, 12.5, f, Stroke::new(1.5, ACCENT.gamma_multiply(0.7)));
    }
    p.galley(pos2(rect.left() + 36.0, rect.top() + 5.0), galley, color);
    resp
}

fn arc(p: &egui::Painter, c: Pos2, r: f32, f: f32, s: Stroke) {
    let n = (40.0 * f).ceil() as usize;
    if n < 1 {
        return;
    }
    let start = -std::f32::consts::FRAC_PI_2;
    let pts = (0..=n)
        .map(|k| {
            let a = start + std::f32::consts::TAU * f * k as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    p.add(Shape::line(pts, s));
}

/// The browser window: an address bar showing the page the user should be on,
/// the console page, and the cursor working through it.
fn window(ui: &Ui, rect: Rect, idx: usize, t: f32) {
    let scene = &SCENES[idx];
    let outer = ui.painter_at(rect.expand(2.0));
    let round = 10u8;
    outer.rect_filled(rect, round, PAGE);
    let bar = Rect::from_min_size(rect.min, vec2(rect.width(), CHROME));
    outer.rect_filled(
        bar,
        CornerRadius {
            nw: round,
            ne: round,
            sw: 0,
            se: 0,
        },
        BAR,
    );
    // Address bar, the host bright and the path quiet, the way browsers show it.
    let pill = Rect::from_min_max(
        pos2(bar.left() + 10.0, bar.top() + 5.0),
        pos2(bar.right() - 10.0, bar.bottom() - 5.0),
    );
    outer.rect_filled(pill, 8.0, FIELD);
    let mut job = egui::text::LayoutJob::default();
    let font = FontId::proportional(10.0);
    job.append("console.tailscale.com", 0.0, fmt(font.clone(), INK));
    job.append(scene.path, 0.0, fmt(font, DIM));
    let g = outer.layout_job(job);
    outer.galley(
        pos2(pill.left() + 10.0, pill.center().y - g.size().y / 2.0),
        g,
        INK,
    );

    let page = Rect::from_min_max(pos2(rect.left(), bar.bottom()), rect.max);
    let p = ui.painter_at(page);
    let m = Mini {
        p: &p,
        o: page.min,
        w: page.width(),
        h: page.height(),
    };
    let n = scene.clicks;
    let done = landed(t, n);
    let targets = match idx {
        0 => list_page(&m, done),
        1 => add_page(&m, done),
        2 => {
            // The menu stays open through the press on All - Read, then closes.
            let closes = LEAD + CLICK + GLIDE + PRESS + 0.15;
            let menu = done == 1 || (done == 2 && t < closes);
            // The highlight follows the pointer onto All - Read as it gets there.
            let lit = if t > LEAD + CLICK + GLIDE * 0.6 { 2 } else { 0 };
            scopes_page(
                &m,
                0.0,
                if menu { 1 } else { done },
                None,
                menu.then_some(lit),
            )
        }
        3 => scopes_page(&m, -SCROLL * ease(t / LEAD), 2, Some(done), None),
        _ => {
            scopes_page(&m, -SCROLL, 2, Some(0), None);
            let bottom = CornerRadius {
                sw: round,
                se: round,
                ..Default::default()
            };
            p.rect_filled(page, bottom, Color32::from_black_alpha(150));
            created_modal(&m, done)
        }
    };
    let start = m.at(m.w * 0.78, m.h * 0.92);
    cursor(&p, t, start, &targets);
    outer.rect_stroke(rect, round, Stroke::new(1.0, LINE), StrokeKind::Inside);
}

fn fmt(font: FontId, color: Color32) -> egui::TextFormat {
    egui::TextFormat {
        font_id: font,
        color,
        ..Default::default()
    }
}

/// Drawing in page coordinates, from the page's top left.
struct Mini<'a> {
    p: &'a egui::Painter,
    o: Pos2,
    w: f32,
    h: f32,
}

impl Mini<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.o + vec2(x, y)
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(self.at(x, y), vec2(w, h))
    }
    fn text(&self, x: f32, y: f32, s: &str, size: f32, c: Color32) -> Rect {
        self.p.text(
            self.at(x, y),
            Align2::LEFT_TOP,
            s,
            FontId::proportional(size),
            c,
        )
    }
    fn bold(&self, x: f32, y: f32, s: &str, size: f32, c: Color32) -> Rect {
        let f = FontId::new(size, FontFamily::Name("strong".into()));
        self.p.text(self.at(x, y), Align2::LEFT_TOP, s, f, c)
    }
    fn mono(&self, x: f32, y: f32, s: &str, c: Color32) -> Rect {
        self.p.text(
            self.at(x, y),
            Align2::LEFT_TOP,
            s,
            FontId::monospace(8.5),
            c,
        )
    }
    /// A text placeholder: a rounded bar where a line of copy sits.
    fn ghost(&self, x: f32, y: f32, w: f32) {
        self.p
            .rect_filled(self.rect(x, y, w, 4.0), 2.0, DIM.gamma_multiply(0.35));
    }
    fn field(&self, r: Rect) {
        self.p.rect_filled(r, 4.0, FIELD.gamma_multiply(0.6));
        self.p
            .rect_stroke(r, 4.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
    }
    /// A console button, filled blue or outlined, with its label centered.
    fn button(&self, r: Rect, label: &str, primary: bool) -> Rect {
        if primary {
            self.p.rect_filled(r, 4.0, BLUE);
        } else {
            self.p.rect_filled(r, 4.0, FIELD);
            self.p
                .rect_stroke(r, 4.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
        }
        self.p.text(
            r.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(9.5),
            if primary { Color32::WHITE } else { INK },
        );
        r
    }
    /// Darkens a button once the cursor has pressed it.
    fn pressed(&self, r: Rect, on: bool) {
        if on {
            self.p.rect_filled(r, 4.0, Color32::from_black_alpha(60));
        }
    }
    fn chevron(&self, x: f32, y: f32, open: bool) {
        let s = Stroke::new(1.2, DIM);
        let pts = if open {
            vec![
                self.at(x - 3.0, y - 1.5),
                self.at(x, y + 1.5),
                self.at(x + 3.0, y - 1.5),
            ]
        } else {
            vec![
                self.at(x - 1.5, y - 3.0),
                self.at(x + 1.5, y),
                self.at(x - 1.5, y + 3.0),
            ]
        };
        self.p.add(Shape::line(pts, s));
    }
    fn checkbox(&self, x: f32, y: f32, on: bool) -> Rect {
        let r = self.rect(x, y, 10.0, 10.0);
        if on {
            self.p.rect_filled(r, 2.0, BLUE);
            let s = Stroke::new(1.4, Color32::WHITE);
            self.p.add(Shape::line(
                vec![
                    self.at(x + 2.2, y + 5.2),
                    self.at(x + 4.2, y + 7.2),
                    self.at(x + 7.8, y + 3.0),
                ],
                s,
            ));
        } else {
            self.p.rect_stroke(
                r,
                2.0,
                Stroke::new(1.0, DIM.gamma_multiply(0.6)),
                StrokeKind::Inside,
            );
        }
        r
    }
    /// Where the content column starts and how wide it runs.
    fn col(&self) -> (f32, f32) {
        let w = (self.w - 56.0).min(440.0);
        ((self.w - w) / 2.0, w)
    }
}

/// Trust credentials, empty, with the Credential button.
fn list_page(m: &Mini, done: usize) -> Vec<Pos2> {
    let (x, w) = m.col();
    m.bold(x, 14.0, "Trust credentials", 15.0, INK);
    m.text(
        x,
        36.0,
        "Provide fine grained access to the Tailscale API using OAuth or OIDC tokens.",
        8.5,
        DIM,
    );
    m.field(m.rect(x, 58.0, w * 0.46, 20.0));
    m.ghost(x + 20.0, 66.0, w * 0.26);
    let f = m.rect(x + w * 0.49, 58.0, 54.0, 20.0);
    m.field(f);
    m.text(f.left() - m.o.x + 10.0, 63.0, "Filters", 9.0, INK);
    let b = m.button(
        m.rect(x + w - 78.0, 58.0, 78.0, 20.0),
        "+  Credential",
        true,
    );
    m.pressed(b, done > 0);
    let t = m.rect(x, 90.0, w, 52.0);
    m.p.rect_stroke(t, 5.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
    for (dx, s) in [
        (14.0, "Client ID"),
        (w * 0.47, "Type"),
        (w * 0.67, "Scopes"),
    ] {
        m.text(x + dx, 96.0, s, 8.0, DIM);
    }
    m.p.hline(t.x_range(), m.o.y + 110.0, Stroke::new(1.0, LINE));
    m.p.text(
        m.at(x + w / 2.0, 126.0),
        Align2::CENTER_CENTER,
        "You do not have any trust credentials",
        FontId::proportional(9.0),
        DIM,
    );
    vec![b.center()]
}

/// New credential, step one: OAuth or OpenID Connect, then Continue.
fn add_page(m: &Mini, done: usize) -> Vec<Pos2> {
    let (x, w) = m.col();
    let w = w.min(330.0);
    let crumb = m.text(x, 10.0, "Trust credentials", 8.5, LINK);
    m.p.hline(
        crumb.x_range(),
        crumb.bottom(),
        Stroke::new(0.6, LINK.gamma_multiply(0.6)),
    );
    m.text(crumb.right() - m.o.x + 3.0, 10.0, "/", 8.5, INK);
    m.bold(x, 26.0, "New credential", 15.0, INK);
    stepper(m, x, 56.0, 1);
    let cw = (w - 8.0) / 2.0;
    for (i, (title, line)) in [
        ("OAuth", "Create an interactive login token"),
        ("OpenID Connect", "Set up a trust relationship"),
    ]
    .into_iter()
    .enumerate()
    {
        let cx = x + i as f32 * (cw + 8.0);
        let r = m.rect(cx, 72.0, cw, 44.0);
        let on = i == 0;
        m.p.rect_stroke(
            r,
            5.0,
            Stroke::new(1.0, if on { BLUE } else { LINE }),
            StrokeKind::Inside,
        );
        let c = m.at(cx + 12.0, 84.0);
        m.p.circle_stroke(c, 4.0, Stroke::new(1.0, if on { BLUE } else { DIM }));
        if on {
            m.p.circle_filled(c, 2.2, BLUE);
        }
        m.text(cx + 22.0, 78.0, title, 9.5, INK);
        m.text(cx + 22.0, 93.0, line, 8.0, DIM);
    }
    m.text(x, 128.0, "Description (optional)", 8.5, INK);
    m.field(m.rect(x, 142.0, w, 20.0));
    let b = m.button(m.rect(x + w - 66.0, 174.0, 66.0, 20.0), "Continue", true);
    m.pressed(b, done > 0);
    vec![b.center()]
}

/// The two-step progress at the top of New credential.
fn stepper(m: &Mini, x: f32, y: f32, at: usize) {
    let mut cx = x + 7.0;
    for (i, label) in ["Settings", "Scopes"].into_iter().enumerate() {
        let on = i + 1 == at;
        let c = m.at(cx, y);
        if on {
            m.p.circle_filled(c, 6.5, BLUE);
        } else {
            m.p.circle_stroke(c, 6.5, Stroke::new(1.0, DIM));
        }
        m.p.text(
            c,
            Align2::CENTER_CENTER,
            (i + 1).to_string(),
            FontId::proportional(8.0),
            if on { Color32::WHITE } else { DIM },
        );
        let l = m.text(cx + 11.0, y - 6.0, label, 9.0, if on { LINK } else { DIM });
        if i == 0 {
            let lx = l.right() + 6.0;
            m.p.hline(
                lx..=lx + 40.0,
                c.y,
                Stroke::new(1.0, DIM.gamma_multiply(0.6)),
            );
            cx = l.right() - m.o.x + 6.0 + 40.0 + 13.0;
        }
    }
}

/// New credential, step two. `done` counts the clicks so far: the Custom scopes
/// menu opened, then All - Read picked, which ticks Read on every scope. `dy`
/// scrolls the page. `generate` makes Generate credential the target, with the
/// clicks landed on it. `menu` opens the Custom scopes menu, with the clicks
/// item to highlight.
fn scopes_page(
    m: &Mini,
    dy: f32,
    done: usize,
    generate: Option<usize>,
    menu: Option<usize>,
) -> Vec<Pos2> {
    let (x, w) = m.col();
    let y = |v: f32| v + dy;
    let all_read = done >= 2;
    m.bold(x, y(10.0), "New credential", 15.0, INK);
    stepper(m, x, y(40.0), 2);
    let card = m.rect(x, y(54.0), w, 34.0);
    m.p.rect_filled(card, 5.0, FIELD.gamma_multiply(0.7));
    m.p.rect_stroke(card, 5.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
    m.bold(x + 10.0, y(59.0), "Scopes", 9.5, INK);
    m.text(
        x + 10.0,
        y(73.0),
        "Choose the type of access this token will have.",
        8.0,
        DIM,
    );
    let dd = m.rect(x + w - 108.0, y(62.0), 98.0, 18.0);
    m.field(dd);
    let choice = if all_read {
        "All - Read"
    } else {
        "Custom scopes"
    };
    m.text(dd.left() - m.o.x + 8.0, y(66.0), choice, 8.5, INK);
    m.chevron(dd.right() - m.o.x - 9.0, y(71.0), true);

    let mut row_y = y(100.0);
    let section = |ry: f32, label: &str, open: bool| {
        m.chevron(x + 3.0, ry + 6.0, open);
        m.text(x + 12.0, ry, label, 9.5, INK);
    };
    section(row_y, "General", true);
    row_y += 16.0;
    let rows = [
        ("DNS", "Read or modify DNS settings."),
        ("Policy File", "Read or modify tailnet policy file."),
        ("Users", "Read user information or modify user roles."),
        ("Tailnets", "Read or create tailnets via API."),
        ("Services", "Read or modify services and their properties."),
        ("OAuth applications", "Read or modify OAuth applications."),
    ];
    let box_h = rows.len() as f32 * 24.0;
    let b = m.rect(x, row_y, w, box_h);
    m.p.rect_stroke(b, 5.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
    let read_x = x + w - 104.0;
    for (i, (name, line)) in rows.iter().enumerate() {
        let ry = row_y + i as f32 * 24.0;
        if i > 0 {
            m.p.hline(b.x_range(), m.o.y + ry, Stroke::new(1.0, LINE));
        }
        m.bold(x + 10.0, ry + 3.0, name, 8.5, INK);
        m.text(x + 10.0, ry + 13.0, line, 7.5, DIM);
        // All - Read ticks Read everywhere and locks it, as the console does.
        m.checkbox(read_x, ry + 7.0, all_read);
        m.text(
            read_x + 14.0,
            ry + 6.0,
            "Read",
            8.5,
            if all_read { DIM } else { INK },
        );
        m.checkbox(read_x + 50.0, ry + 7.0, false);
        m.text(read_x + 64.0, ry + 6.0, "Write", 8.5, INK);
    }
    row_y += box_h + 10.0;
    for label in ["Devices", "Keys", "Logging", "Settings"] {
        section(row_y, label, false);
        row_y += 18.0;
    }
    let by = row_y + 6.0;
    m.button(m.rect(x + w - 178.0, by, 56.0, 20.0), "Back", false);
    let g = m.button(
        m.rect(x + w - 116.0, by, 116.0, 20.0),
        "Generate credential",
        true,
    );
    if let Some(pressed) = generate {
        m.pressed(g, pressed > 0);
        return vec![g.center()];
    }

    // The menu, open between the two clicks, over the rows below it.
    let open = menu.is_some();
    let lit = menu.unwrap_or(0);
    let menu = m.rect(dd.right() - m.o.x - 140.0, y(84.0), 140.0, 120.0);
    if open {
        m.p.rect_filled(menu, 6.0, Color32::from_rgb(36, 35, 35));
        m.p.rect_stroke(menu, 6.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
    }
    let mut targets = vec![dd.center()];
    for (i, (title, line)) in [
        ("Custom scopes", "Pick and choose your own access"),
        ("All - Read & Write", "Complete access to all endpoints"),
        ("All - Read", "Read access to all endpoints"),
    ]
    .into_iter()
    .enumerate()
    {
        let item = Rect::from_min_size(
            menu.min + vec2(4.0, 4.0 + i as f32 * 38.0),
            vec2(menu.width() - 8.0, 36.0),
        );
        if i == 2 {
            targets.push(item.center());
        }
        if !open {
            continue;
        }
        if i == lit {
            m.p.rect_filled(item, 4.0, FIELD);
        }
        let ix = item.left() - m.o.x + 8.0;
        let iy = item.top() - m.o.y;
        m.text(ix, iy + 6.0, title, 9.0, INK);
        m.text(ix, iy + 19.0, line, 7.5, DIM);
    }
    targets
}

/// Credential created: the one moment the secret is shown in full.
fn created_modal(m: &Mini, done: usize) -> Vec<Pos2> {
    let w = 270.0;
    let x = (m.w - w) / 2.0;
    let r = m.rect(x, 14.0, w, 168.0);
    m.p.rect_filled(r, 7.0, Color32::from_rgb(36, 35, 35));
    m.p.rect_stroke(r, 7.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
    let ix = x + 14.0;
    m.bold(ix, 26.0, "Credential created", 11.0, INK);
    m.text(
        ix,
        44.0,
        "Be sure to copy the client secret below.",
        8.5,
        INK.gamma_multiply(0.85),
    );
    m.text(
        ix,
        55.0,
        "It won't be shown in full again.",
        8.5,
        INK.gamma_multiply(0.85),
    );
    let fw = w - 28.0;
    m.bold(ix, 72.0, "Client ID", 8.5, INK);
    let id = m.rect(ix, 85.0, fw, 20.0);
    m.field(id);
    m.mono(ix + 8.0, 90.0, "kXXXXXXXXX11CNTRL", INK);
    m.bold(ix, 112.0, "Client secret", 8.5, INK);
    let sec = m.rect(ix, 125.0, fw, 20.0);
    m.field(sec);
    m.mono(
        ix + 8.0,
        130.0,
        "tskey-client-kXXXXXXXXX11CNTRL-XXXXXX",
        INK,
    );
    let copied = done > 0;
    let copy = m.at(ix + fw - 12.0, 135.0);
    paint_icon(
        m.p,
        copy,
        if copied { icon::CHECK } else { icon::COPY },
        10.0,
        if copied { GREEN } else { INK },
    );
    if copied {
        m.p.text(
            m.at(ix, 150.0),
            Align2::LEFT_TOP,
            "Copied",
            FontId::proportional(8.0),
            GREEN,
        );
    }
    m.button(m.rect(x + w - 54.0, 156.0, 40.0, 18.0), "Done", false);
    vec![copy]
}

/// The pointer, gliding from target to target and pressing each, with a ring
/// that spreads from every press.
fn cursor(p: &egui::Painter, t: f32, start: Pos2, targets: &[Pos2]) {
    if targets.is_empty() {
        return;
    }
    for (i, &tg) in targets.iter().enumerate() {
        let f = (t - LEAD - i as f32 * CLICK - GLIDE) / 0.6;
        if (0.0..1.0).contains(&f) {
            p.circle_stroke(
                tg,
                4.0 + 14.0 * ease(f),
                Stroke::new(1.6, Color32::WHITE.gamma_multiply(0.8 * (1.0 - f))),
            );
        }
    }
    let seg = ((t - LEAD) / CLICK).floor().max(0.0) as usize;
    let (pos, pressed) = if t < LEAD {
        (start, false)
    } else if seg >= targets.len() {
        (targets[targets.len() - 1], false)
    } else {
        let local = t - LEAD - seg as f32 * CLICK;
        let from = if seg == 0 { start } else { targets[seg - 1] };
        let pos = from.lerp(targets[seg], ease(local / GLIDE));
        (pos, (GLIDE..GLIDE + PRESS).contains(&local))
    };
    // The tip rests just off the target's center, so its label stays readable.
    arrow(p, pos + vec2(5.0, 3.0), if pressed { 0.86 } else { 1.0 });
}

fn arrow(p: &egui::Painter, tip: Pos2, s: f32) {
    let pt = |x: f32, y: f32| tip + vec2(x, y) * s;
    let head = vec![pt(0.0, 0.0), pt(0.0, 14.0), pt(3.6, 10.6), pt(10.0, 10.0)];
    let tail = vec![pt(3.2, 10.0), pt(6.0, 16.2), pt(8.4, 15.2), pt(5.8, 9.4)];
    p.add(Shape::convex_polygon(head, Color32::WHITE, Stroke::NONE));
    p.add(Shape::convex_polygon(tail, Color32::WHITE, Stroke::NONE));
    let outline = vec![
        pt(0.0, 0.0),
        pt(0.0, 14.0),
        pt(3.6, 10.6),
        pt(6.0, 16.2),
        pt(8.4, 15.2),
        pt(6.0, 10.0),
        pt(10.0, 10.0),
        pt(0.0, 0.0),
    ];
    p.add(Shape::line(outline, Stroke::new(1.0, Color32::BLACK)));
}
