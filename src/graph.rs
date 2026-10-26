//! "Who can talk to whom" as rings: this device in the middle, online devices
//! on a ring around it, offline ones faded on an outer ring. Nodes spring to
//! their slot, so a dragged one glides back.

use std::collections::HashMap;

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2, pos2, vec2,
};

use crate::access::{Access, Matrix, Source};
use crate::theme::*;
use crate::ts::Snapshot;

#[derive(Default)]
pub struct GraphState {
    pos: HashMap<String, Vec2>,
    vel: HashMap<String, Vec2>,
    dragging: Option<String>,
    cam: Camera,
    /// Set once the user pans or zooms; until then the camera keeps fitting the graph.
    manual: bool,
    /// When the pointer last moved the camera, for the return-home delay.
    last_input: f64,
    pub hovered: Option<String>,
    /// Test hook state: `TSDU_PAN_AT=<seconds>` throws the camera off screen once.
    test_pan_done: bool,
    test_pan_at: Option<Option<f64>>,
}

/// Pan and zoom, each driven by a spring toward a target so recentering glides.
#[derive(Default)]
struct Camera {
    pan: Vec2,
    zoom: f32,
    pan_vel: Vec2,
    zoom_vel: f32,
}

impl Camera {
    fn spring_to(&mut self, pan: Vec2, zoom: f32, dt: f32) -> bool {
        // Slightly underdamped, so a long return ends with a small settle.
        const W: f32 = 9.0;
        const ZETA: f32 = 0.8;
        let steps = (dt / 0.004).ceil().max(1.0) as usize;
        let h = dt / steps as f32;
        for _ in 0..steps {
            let a = (pan - self.pan) * W * W - self.pan_vel * 2.0 * ZETA * W;
            self.pan_vel += a * h;
            self.pan += self.pan_vel * h;
            let az = (zoom - self.zoom) * W * W - self.zoom_vel * 2.0 * ZETA * W;
            self.zoom_vel += az * h;
            self.zoom += self.zoom_vel * h;
        }
        (pan - self.pan).length() > 0.5
            || self.pan_vel.length() > 1.0
            || (zoom - self.zoom).abs() > 0.002
    }

    fn stop(&mut self) {
        self.pan_vel = Vec2::ZERO;
        self.zoom_vel = 0.0;
    }
}

const NODE_R: f32 = 20.0;
/// How long the graph may sit fully off screen before the camera brings it back.
const RETURN_AFTER: f64 = 1.0;
/// Room kept clear for the title above and the legend strip below.
const TITLE_H: f32 = 72.0;
const LEGEND_H: f32 = 64.0;

/// The online ring grows with the number of devices on it; offline ones sit
/// one step further out.
fn ring_radius(online: usize) -> f32 {
    (online as f32 * 32.0).clamp(130.0, 240.0)
}
const OUTER_STEP: f32 = 105.0;

/// Where each node belongs: this device in the middle, online peers evenly on
/// the inner ring in `order`, offline peers on the outer ring between them.
fn slots(center: &[bool], online: &[bool], order: &[usize]) -> Vec<Vec2> {
    let mut out = vec![Vec2::ZERO; center.len()];
    let inner: Vec<usize> = order
        .iter()
        .copied()
        .filter(|&i| !center[i] && online[i])
        .collect();
    let outer: Vec<usize> = order
        .iter()
        .copied()
        .filter(|&i| !center[i] && !online[i])
        .collect();
    let r1 = ring_radius(inner.len());
    let place = |list: &[usize], r: f32, offset: f32, out: &mut Vec<Vec2>| {
        for (k, &i) in list.iter().enumerate() {
            let a = (k as f32 + offset) / list.len().max(1) as f32 * std::f32::consts::TAU
                - std::f32::consts::FRAC_PI_2;
            out[i] = vec2(a.cos(), a.sin()) * r;
        }
    };
    place(&inner, r1, 0.0, &mut out);
    place(&outer, r1 + OUTER_STEP, 0.5, &mut out);
    out
}

impl GraphState {
    /// Moves every node a step toward its slot; returns how much is still moving.
    fn step(&mut self, ids: &[String], target: &[Vec2], dt: f32) -> f32 {
        let mut energy = 0.0;
        for (i, id) in ids.iter().enumerate() {
            let p = *self.pos.entry(id.clone()).or_insert(target[i]);
            let v = self.vel.entry(id.clone()).or_default();
            if self.dragging.as_deref() == Some(id) {
                *v = Vec2::ZERO;
                continue;
            }
            // A critically damped spring, stepped finely so a long frame stays stable.
            const W: f32 = 10.0;
            let steps = (dt / 0.004).ceil().max(1.0) as usize;
            let h = dt / steps as f32;
            let mut p = p;
            for _ in 0..steps {
                let a = (target[i] - p) * W * W - *v * 2.0 * W;
                *v += a * h;
                p += *v * h;
            }
            energy += v.length() + (target[i] - p).length();
            self.pos.insert(id.clone(), p);
        }
        energy
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Both,
    OneWay,
}

pub fn show(
    ui: &mut egui::Ui,
    st: &mut GraphState,
    snap: &Snapshot,
    m: &Matrix,
    selected: &mut Option<String>,
    mask: &crate::privacy::Mask,
    icons: &HashMap<String, crate::osicon::Icon>,
) {
    if st.cam.zoom == 0.0 {
        st.cam.zoom = 1.0;
    }
    let nodes: Vec<&crate::ts::Node> = snap.all().collect();
    let ids: Vec<String> = nodes.iter().map(|n| n.stable_id.clone()).collect();
    let idx: HashMap<&str, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let center: Vec<bool> = nodes.iter().map(|n| n.is_self).collect();
    let online: Vec<bool> = nodes.iter().map(|n| n.online || n.is_self).collect();
    // Around each ring, devices of the same kind (owner, or tag set) sit together.
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&i| {
        let n = nodes[i];
        let kind = if n.tags.is_empty() {
            format!("0{}", n.user_login)
        } else {
            format!("1{}", n.tags.join(","))
        };
        (!n.is_self, kind, mask.order(n))
    });
    let target = slots(&center, &online, &order);

    // One visual edge per pair, with its direction.
    struct Edge {
        a: usize,
        b: usize,
        kind: Kind,
        weak: bool,
        unknown_back: bool,
    }
    let mut edges = Vec::new();
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            let ab = m.get(&ids[i], &ids[j]);
            let ba = m.get(&ids[j], &ids[i]);
            let weak_of = |x: &Access| {
                x.source() != Some(Source::Exact) && x.source() != Some(Source::Probed)
            };
            match (ab.allowed(), ba.allowed()) {
                (Some(true), Some(true)) => edges.push(Edge {
                    a: i,
                    b: j,
                    kind: Kind::Both,
                    weak: weak_of(&ab) && weak_of(&ba),
                    unknown_back: false,
                }),
                (Some(true), other) => edges.push(Edge {
                    a: i,
                    b: j,
                    kind: Kind::OneWay,
                    weak: weak_of(&ab),
                    unknown_back: other.is_none(),
                }),
                (other, Some(true)) => edges.push(Edge {
                    a: j,
                    b: i,
                    kind: Kind::OneWay,
                    weak: weak_of(&ba),
                    unknown_back: other.is_none(),
                }),
                _ => {}
            }
        }
    }

    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let rect = resp.rect;
    painter.rect_filled(rect, 0.0, BG);
    let dt = ui.input(|i| i.stable_dt).min(0.05);
    let now = ui.input(|i| i.time);
    let energy = st.step(&ids, &target, dt);
    let node_r = |zoom: f32| NODE_R * zoom.clamp(0.6, 1.6);

    // Camera first, so hit-testing below runs against what gets drawn. Free while
    // the user steers it, but once the whole graph has been off screen for a
    // moment with nothing held, it springs back home.
    let at = *st.test_pan_at.get_or_insert_with(|| {
        std::env::var("TSDU_PAN_AT")
            .ok()
            .and_then(|s| s.parse().ok())
    });
    if !st.test_pan_done
        && let Some(at) = at
        && now >= at
    {
        st.test_pan_done = true;
        st.cam.pan += vec2(2500.0, 0.0);
        st.cam.stop();
        st.manual = true;
        st.last_input = now;
    }
    let mut bb = Rect::NOTHING;
    for id in &ids {
        bb.extend_with(st.pos[id].to_pos2());
    }
    // On screen, the graph is its node boxes plus everything between them, so
    // zooming into the empty middle of the ring still counts as in view.
    let on_screen = Rect::from_min_max(
        rect.center() + st.cam.pan + bb.min.to_vec2() * st.cam.zoom,
        rect.center() + st.cam.pan + bb.max.to_vec2() * st.cam.zoom,
    )
    .expand(node_r(st.cam.zoom));
    let in_view = ids.is_empty() || rect.expand(-4.0).intersects(on_screen);
    let holding = ui.input(|i| i.pointer.any_down());
    if st.manual && !in_view && !holding && now - st.last_input > RETURN_AFTER {
        st.manual = false;
    }
    let mut camera_moving = false;
    // A node being dragged moves the fit target; hold the camera still meanwhile.
    if !st.manual && !ids.is_empty() && st.dragging.is_none() {
        // Fit into the canvas above the legend strip, with room for the labels that
        // hang under each node and spread past it sideways.
        let avail = vec2(
            rect.width() - 60.0,
            rect.height() - TITLE_H - LEGEND_H - 24.0,
        );
        // The rings are fitted, not where nodes happen to be mid-glide.
        let mut tb = Rect::NOTHING;
        for t in &target {
            tb.extend_with(t.to_pos2());
        }
        let span = tb.size() + vec2(150.0, 90.0);
        let fit = (avail.x / span.x).min(avail.y / span.y).clamp(0.4, 1.3);
        // Labels make the drawn graph bottom heavy, so aim a little below its middle.
        let look = tb.center().to_vec2() + vec2(0.0, 14.0);
        let pan = -look * fit + vec2(0.0, (TITLE_H - LEGEND_H) / 2.0);
        camera_moving = st.cam.spring_to(pan, fit, dt);
    }

    let origin = rect.center() + st.cam.pan;
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let hit = pointer.and_then(|p| {
        (0..ids.len()).find(|&i| {
            (origin + st.pos[&ids[i]] * st.cam.zoom).distance(p) < node_r(st.cam.zoom) + 4.0
        })
    });
    st.hovered = hit.map(|i| ids[i].clone());

    if resp.drag_started() {
        st.dragging = hit.map(|i| ids[i].clone());
    }
    if resp.dragged() {
        let d = resp.drag_delta();
        match &st.dragging {
            Some(id) => {
                *st.pos.get_mut(id).unwrap() += d / st.cam.zoom;
                st.vel.insert(id.clone(), Vec2::ZERO);
            }
            None => {
                st.cam.pan += d;
                st.cam.stop();
                st.manual = true;
                st.last_input = now;
            }
        }
    }
    if resp.drag_stopped() {
        st.dragging = None;
    }
    if resp.clicked() {
        *selected = hit.map(|i| ids[i].clone());
    }
    if resp.double_clicked() && hit.is_none() {
        st.manual = false;
    }
    if resp.hovered() {
        let zoom_delta = ui.input(|i| i.zoom_delta());
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        let factor = zoom_delta * (scroll * 0.0015).exp();
        if factor != 1.0
            && let Some(p) = pointer
        {
            let before = (p - origin) / st.cam.zoom;
            st.cam.zoom = (st.cam.zoom * factor).clamp(0.35, 2.5);
            st.cam.pan += (p - origin) - before * st.cam.zoom;
            st.cam.stop();
            st.manual = true;
            st.last_input = now;
        }
    }
    if hit.is_some() {
        ui.ctx().set_cursor_icon(if st.dragging.is_some() {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::PointingHand
        });
    }

    let origin = rect.center() + st.cam.pan;
    let zoom = st.cam.zoom;
    // Dot grid backdrop, thinned when zoomed out so it never turns into a carpet.
    let mut grid = 28.0 * zoom;
    while grid < 18.0 {
        grid *= 2.0;
    }
    let gx0 = ((rect.left() - origin.x) / grid).floor() as i32;
    let gy0 = ((rect.top() - origin.y) / grid).floor() as i32;
    for gx in gx0..=gx0 + (rect.width() / grid) as i32 + 1 {
        for gy in gy0..=gy0 + (rect.height() / grid) as i32 + 1 {
            let p = origin + vec2(gx as f32, gy as f32) * grid;
            if rect.contains(p) {
                painter.circle_filled(p, 1.0, Color32::from_rgb(0x1d, 0x21, 0x29));
            }
        }
    }
    let screen: Vec<Pos2> = ids.iter().map(|id| origin + st.pos[id] * zoom).collect();
    let me = center.iter().position(|c| *c);

    // A faint guide for the online ring, so the layout reads as a ring and not
    // as scattered dots.
    let inner_n = (0..ids.len()).filter(|&i| !center[i] && online[i]).count();
    if inner_n > 0 {
        let r1 = ring_radius(inner_n);
        painter.circle_stroke(
            origin,
            r1 * zoom,
            Stroke::new(1.0, BORDER.gamma_multiply(0.8)),
        );
    }

    let focus = st.hovered.clone().or(selected.clone());
    let focus_i = focus.as_ref().and_then(|f| idx.get(f.as_str()).copied());
    let t = ui.input(|i| i.time) as f32;

    // Edges. At rest only this device's own links show, as quiet spokes that
    // never cross; a one-way link carries a small arrowhead. Focusing a device
    // brings out all of its links, with direction and blocking made loud.
    let mut animating = false;
    for e in &edges {
        let (pa, pb) = (screen[e.a], screen[e.b]);
        let dir = (pb - pa).normalized();
        // Same radius the nodes are drawn with; tagged squares reach a bit further.
        let rad = |i: usize| node_r(zoom) * if nodes[i].tags.is_empty() { 1.0 } else { 1.15 };
        let (s, d) = (pa + dir * (rad(e.a) + 3.0), pb - dir * (rad(e.b) + 4.0));
        if (d - s).dot(dir) <= 0.0 {
            continue;
        }
        let lit = focus_i.is_some_and(|f| f == e.a || f == e.b);
        let spoke = me.is_some_and(|m| m == e.a || m == e.b);
        if !lit && !spoke {
            continue;
        }
        let faded = focus_i.is_some() && !lit;
        let col = match (lit, e.kind) {
            (true, Kind::OneWay) => AMBER,
            (true, Kind::Both) => MUTED,
            _ => EDGE,
        };
        let mut alpha = if faded { 0.15 } else { 1.0 };
        if e.weak && !lit {
            alpha *= 0.6;
        }
        if !lit && (!online[e.a] || !online[e.b]) {
            alpha *= 0.5;
        }
        let col = col.gamma_multiply(alpha);
        let width = if lit { 2.0 } else { 1.2 };
        if e.weak {
            painter.extend(Shape::dashed_line(
                &[s, d],
                Stroke::new(width, col),
                7.0,
                5.0,
            ));
        } else {
            painter.line_segment([s, d], Stroke::new(width, col));
        }
        // Direction marks sit mid-line, where node labels never cover them.
        let mid = s + (d - s) * 0.55;
        match e.kind {
            Kind::OneWay if lit => {
                arrow(&painter, mid + dir * 6.0, dir, col, 12.0);
                // Packets flowing in the allowed direction.
                let len = (d - s).length();
                if len > 30.0 {
                    animating = true;
                    for k in 0..3 {
                        let f = ((t * 0.45 + k as f32 / 3.0) % 1.0) * len;
                        painter.circle_filled(s + dir * f, 2.6, col);
                    }
                }
                if !e.unknown_back {
                    // Blocked direction: a small bar at the source end.
                    let n = vec2(-dir.y, dir.x);
                    let p = s + dir * 10.0;
                    painter.line_segment([p - n * 6.0, p + n * 6.0], Stroke::new(2.0, RED));
                }
            }
            Kind::OneWay => {
                arrow(
                    &painter,
                    mid + dir * 4.0,
                    dir,
                    AMBER.gamma_multiply(0.75 * alpha),
                    8.0,
                );
            }
            Kind::Both => {}
        }
    }

    // Nodes.
    for (i, n) in nodes.iter().enumerate() {
        let p = screen[i];
        let r = node_r(zoom);
        let dim = focus_i.is_some_and(|f| {
            f != i
                && !edges
                    .iter()
                    .any(|e| (e.a == f && e.b == i) || (e.b == f && e.a == i))
        });
        let badge = Badge::of(n);
        let online = badge.online;
        let base = if online { 1.0 } else { 0.55 };
        let a = if dim { 0.25 } else { base };
        let tagged = !n.tags.is_empty();
        if selected.as_deref() == Some(&n.stable_id) || st.hovered.as_deref() == Some(&n.stable_id)
        {
            let halo = if online { GREEN } else { MUTED };
            painter.circle_filled(p, r + 11.0, halo.gamma_multiply(0.14 * a));
        }
        let glyph = icons
            .get(&n.stable_id)
            .map_or(crate::osicon::UNKNOWN, |i| i.glyph);
        paint_badge(&painter, p, r, glyph, badge, a);
        let label = if n.is_self {
            format!("{} (you)", mask.name(n))
        } else {
            mask.name(n)
        };
        let col = if online { TEXT } else { MUTED };
        let g = painter.layout_no_wrap(label, FontId::proportional(13.0), col.gamma_multiply(a));
        let lr = Rect::from_center_size(
            p + vec2(0.0, r + 8.0 + g.size().y / 2.0),
            g.size() + vec2(12.0, 4.0),
        );
        painter.rect_filled(lr, 6.0, BG.gamma_multiply(0.85 * a));
        painter.galley(lr.min + vec2(6.0, 2.0), g, TEXT);
        if tagged {
            painter.text(
                p + vec2(0.0, r + 28.0),
                Align2::CENTER_TOP,
                n.tags.join(" "),
                FontId::proportional(ty::SMALL - 1.0),
                PURPLE.gamma_multiply(a),
            );
        }
    }

    title(&painter, rect);
    legend(&painter, rect);

    // Hover card for the pair under focus.
    if let (Some(h), Some(p)) = (hit, pointer) {
        let n = nodes[h];
        // What the device is first, then who it can reach.
        let mut lines = vec![];
        if let Some(i) = icons.get(&n.stable_id) {
            lines.push(i.label.clone());
            lines.push(String::new());
        }
        for (j, o) in nodes.iter().enumerate() {
            if j == h {
                continue;
            }
            let out = m.get(&n.stable_id, &o.stable_id).allowed();
            let inn = m.get(&o.stable_id, &n.stable_id).allowed();
            let sym = match (out, inn) {
                (Some(true), Some(true)) => "<->",
                (Some(true), _) => " ->",
                (_, Some(true)) => "<- ",
                (None, None) => " ? ",
                _ => " x ",
            };
            lines.push(format!("{sym}  {}", mask.name(o)));
        }
        let text = lines.join("\n");
        let galley = painter.layout_no_wrap(text, FontId::monospace(12.0), TEXT);
        let pos = p + vec2(18.0, 12.0);
        let r = Rect::from_min_size(pos, galley.size() + vec2(20.0, 16.0));
        let r = r.translate(vec2(
            (rect.right() - r.right()).min(0.0),
            (rect.bottom() - r.bottom()).min(0.0),
        ));
        painter.rect_filled(r, RADIUS, SURFACE);
        painter.rect_stroke(
            r,
            RADIUS,
            Stroke::new(1.0, BORDER),
            egui::StrokeKind::Inside,
        );
        painter.galley(r.min + vec2(10.0, 8.0), galley, TEXT);
    }

    // A pending return home needs frames to notice the delay running out.
    let waiting_home = st.manual && !in_view;
    if energy > 0.3 || animating || camera_moving || waiting_home || st.dragging.is_some() {
        ui.ctx().request_repaint();
    }
}

fn arrow(painter: &egui::Painter, tip: Pos2, dir: Vec2, col: Color32, size: f32) {
    let n = vec2(-dir.y, dir.x);
    let back = tip - dir * size;
    painter.add(Shape::convex_polygon(
        vec![tip, back + n * size * 0.5, back - n * size * 0.5],
        col,
        Stroke::NONE,
    ));
}

/// The page title, drawn over the canvas like every other screen's.
fn title(painter: &egui::Painter, rect: Rect) {
    let x = rect.left() + sp::XL;
    let y = rect.top() + sp::L + 4.0 + 16.0;
    let g = painter.layout_no_wrap(
        "Access graph".to_owned(),
        FontId::new(ty::HEADING, egui::FontFamily::Name("strong".into())),
        TEXT,
    );
    let w = g.size().x;
    painter.galley(pos2(x, y - g.size().y / 2.0), g, TEXT);
    painter.text(
        pos2(x + w + sp::S + sp::XS, y + 1.0),
        Align2::LEFT_CENTER,
        "Who can open connections to whom",
        FontId::proportional(ty::BODY),
        MUTED,
    );
}

/// One strip along the bottom: a mark and a word for each kind of link and node.
fn legend(painter: &egui::Painter, rect: Rect) {
    let font = FontId::proportional(ty::SMALL);
    let mini = |c: Pos2, b: Badge| paint_badge(painter, c, 6.0, "", b, 1.0);
    let plain = Badge {
        online: true,
        me: false,
        tagged: false,
    };
    type Mark<'a> = Box<dyn Fn(Pos2) + 'a>;
    let items: Vec<(f32, Mark, &str)> = vec![
        (
            22.0,
            Box::new(|c: Pos2| {
                painter.line_segment(
                    [c - vec2(11.0, 0.0), c + vec2(11.0, 0.0)],
                    Stroke::new(1.4, MUTED),
                );
            }),
            "Both ways",
        ),
        (
            22.0,
            Box::new(|c: Pos2| {
                painter.line_segment(
                    [c - vec2(11.0, 0.0), c + vec2(11.0, 0.0)],
                    Stroke::new(1.4, EDGE),
                );
                arrow(painter, c + vec2(6.0, 0.0), vec2(1.0, 0.0), AMBER, 8.0);
            }),
            "One way",
        ),
        (
            10.0,
            Box::new(|c: Pos2| {
                painter.line_segment(
                    [c - vec2(0.0, 6.0), c + vec2(0.0, 6.0)],
                    Stroke::new(2.0, RED),
                );
            }),
            "Blocked",
        ),
        (
            22.0,
            Box::new(|c: Pos2| {
                painter.extend(Shape::dashed_line(
                    &[c - vec2(11.0, 0.0), c + vec2(11.0, 0.0)],
                    Stroke::new(1.4, MUTED),
                    5.0,
                    3.0,
                ));
            }),
            "Inferred",
        ),
        (14.0, Box::new(move |c: Pos2| mini(c, plain)), "Online"),
        (
            14.0,
            Box::new(move |c: Pos2| {
                mini(
                    c,
                    Badge {
                        online: false,
                        ..plain
                    },
                )
            }),
            "Offline",
        ),
        (
            14.0,
            Box::new(move |c: Pos2| {
                mini(
                    c,
                    Badge {
                        tagged: true,
                        ..plain
                    },
                )
            }),
            "Tagged",
        ),
    ];
    let widths: Vec<f32> = items
        .iter()
        .map(|(mw, _, label)| {
            mw + 6.0
                + painter
                    .layout_no_wrap(label.to_string(), font.clone(), MUTED)
                    .size()
                    .x
        })
        .collect();
    let gap = sp::L + 4.0;
    let total: f32 = widths.iter().sum::<f32>() + gap * (items.len() - 1) as f32 + sp::L * 2.0;
    let strip = Rect::from_min_size(
        pos2(rect.left() + sp::XL, rect.bottom() - sp::L - 32.0),
        vec2(total, 32.0),
    );
    painter.rect_filled(strip, RADIUS, SURFACE.gamma_multiply(0.94));
    painter.rect_stroke(
        strip,
        RADIUS,
        Stroke::new(1.0, BORDER),
        egui::StrokeKind::Inside,
    );
    let mut x = strip.left() + sp::L;
    let y = strip.center().y;
    for ((mw, mark, label), w) in items.iter().zip(widths) {
        mark(pos2(x + mw / 2.0, y));
        crate::widgets::paint_text(painter, pos2(x + mw + 6.0, y), label, ty::SMALL, MUTED);
        x += w + gap;
    }
}
