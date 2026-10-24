//! Force-directed "who can talk to whom" graph, Obsidian style.

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
    pan: Vec2,
    zoom: f32,
    /// Set once the user pans or zooms; until then the view keeps fitting the graph.
    manual: bool,
    pub hovered: Option<String>,
}

const NODE_R: f32 = 20.0;

fn seed(i: usize, n: usize) -> Vec2 {
    let a = i as f32 / n.max(1) as f32 * std::f32::consts::TAU;
    vec2(a.cos(), a.sin()) * 220.0
}

impl GraphState {
    fn step(&mut self, ids: &[String], links: &[(usize, usize)], dt: f32) -> f32 {
        let n = ids.len();
        for (i, id) in ids.iter().enumerate() {
            self.pos.entry(id.clone()).or_insert_with(|| seed(i, n));
            self.vel.entry(id.clone()).or_default();
        }
        let p: Vec<Vec2> = ids.iter().map(|id| self.pos[id]).collect();
        let mut f = vec![Vec2::ZERO; n];
        for i in 0..n {
            for j in (i + 1)..n {
                let d = p[i] - p[j];
                let dist2 = d.length_sq().max(100.0);
                let push = d.normalized() * (160000.0 / dist2);
                f[i] += push;
                f[j] -= push;
            }
            f[i] -= p[i] * 0.012;
        }
        for &(a, b) in links {
            let d = p[b] - p[a];
            let pull = d.normalized() * (d.length() - 230.0) * 0.02;
            f[a] += pull;
            f[b] -= pull;
        }
        let mut energy = 0.0;
        for (i, id) in ids.iter().enumerate() {
            if self.dragging.as_deref() == Some(id) {
                continue;
            }
            let v = self.vel.get_mut(id).unwrap();
            *v = (*v + f[i] * dt * 60.0) * 0.82;
            if v.length() > 40.0 {
                *v = v.normalized() * 40.0;
            }
            energy += v.length();
            *self.pos.get_mut(id).unwrap() += *v * dt * 60.0;
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
) {
    if st.zoom == 0.0 {
        st.zoom = 1.0;
    }
    let nodes: Vec<&crate::ts::Node> = snap.all().collect();
    let ids: Vec<String> = nodes.iter().map(|n| n.stable_id.clone()).collect();
    let idx: HashMap<&str, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();

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
    let links: Vec<(usize, usize)> = edges.iter().map(|e| (e.a, e.b)).collect();

    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let rect = resp.rect;
    painter.rect_filled(rect, 0.0, BG);
    // Dot grid backdrop.
    if !st.manual && !ids.is_empty() {
        let mut bb = Rect::NOTHING;
        for id in &ids {
            if let Some(p) = st.pos.get(id) {
                bb.extend_with(p.to_pos2());
            }
        }
        if bb.is_positive() || bb.width() > 0.0 {
            let fit = ((rect.width() - 220.0) / bb.width().max(1.0))
                .min((rect.height() - 200.0) / bb.height().max(1.0))
                .clamp(0.4, 1.8);
            let pan = -(bb.center().to_vec2()) * fit + vec2(0.0, -10.0);
            st.zoom += (fit - st.zoom) * 0.15;
            st.pan += (pan - st.pan) * 0.15;
        }
    }
    let grid = 28.0 * st.zoom;
    let origin = rect.center() + st.pan;
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

    let dt = ui.input(|i| i.stable_dt).min(0.05);
    let energy = st.step(&ids, &links, dt);
    let to_screen = |v: Vec2, st: &GraphState| origin + v * st.zoom;
    let screen: Vec<Pos2> = ids.iter().map(|id| to_screen(st.pos[id], st)).collect();

    // Hit-testing.
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let hit = pointer.and_then(|p| {
        (0..ids.len()).find(|&i| screen[i].distance(p) < NODE_R * st.zoom.max(0.6) + 4.0)
    });
    st.hovered = hit.map(|i| ids[i].clone());

    if resp.drag_started() {
        st.dragging = hit.map(|i| ids[i].clone());
    }
    if resp.dragged() {
        let d = resp.drag_delta();
        match &st.dragging {
            Some(id) => {
                *st.pos.get_mut(id).unwrap() += d / st.zoom;
                st.vel.insert(id.clone(), Vec2::ZERO);
            }
            None => {
                st.pan += d;
                st.manual = true;
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
        if factor != 1.0 {
            if let Some(p) = pointer {
                let before = (p - origin) / st.zoom;
                st.zoom = (st.zoom * factor).clamp(0.35, 2.5);
                st.manual = true;
                st.pan += (p - origin) - before * st.zoom;
            }
        }
    }
    if hit.is_some() {
        ui.ctx().set_cursor_icon(if st.dragging.is_some() {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::PointingHand
        });
    }

    let focus = st.hovered.clone().or(selected.clone());
    let focus_i = focus.as_ref().and_then(|f| idx.get(f.as_str()).copied());
    let t = ui.input(|i| i.time) as f32;

    // Edges.
    for e in &edges {
        let (pa, pb) = (screen[e.a], screen[e.b]);
        let dir = (pb - pa).normalized();
        // Same radius the nodes are drawn with; tagged squares reach a bit further.
        let rad = |i: usize| {
            NODE_R * st.zoom.clamp(0.6, 1.6) * if nodes[i].tags.is_empty() { 1.0 } else { 1.15 }
        };
        let (s, d) = (pa + dir * (rad(e.a) + 3.0), pb - dir * (rad(e.b) + 4.0));
        let touches = focus_i.is_none_or(|f| f == e.a || f == e.b);
        let base = match e.kind {
            Kind::Both => Color32::from_rgb(0x4a, 0x55, 0x68),
            Kind::OneWay => AMBER,
        };
        let mut alpha = if touches { 1.0 } else { 0.12 };
        if e.weak {
            alpha *= if e.kind == Kind::Both { 0.45 } else { 0.75 };
        }
        let col = base.gamma_multiply(alpha);
        let width = if e.kind == Kind::OneWay { 2.0 } else { 1.4 }
            * if touches && focus_i.is_some() {
                1.4
            } else {
                1.0
            };
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
        match e.kind {
            Kind::OneWay => {
                arrow(&painter, d, dir, col, 11.0);
                // Packets flowing in the allowed direction.
                let len = (d - s).length();
                if len > 30.0 {
                    for k in 0..3 {
                        let f = ((t * 0.45 + k as f32 / 3.0) % 1.0) * len;
                        painter.circle_filled(s + dir * f, 2.6, col);
                    }
                }
                if !e.unknown_back {
                    // Blocked direction: a small bar at the source end.
                    let n = vec2(-dir.y, dir.x);
                    let p = s + dir * 10.0;
                    painter.line_segment(
                        [p - n * 6.0, p + n * 6.0],
                        Stroke::new(2.0, RED.gamma_multiply(alpha)),
                    );
                }
            }
            Kind::Both => {
                if focus_i.is_some() && touches {
                    arrow(&painter, d, dir, col, 8.0);
                    arrow(&painter, s, -dir, col, 8.0);
                }
            }
        }
    }

    // Nodes.
    for (i, n) in nodes.iter().enumerate() {
        let p = screen[i];
        let r = NODE_R * st.zoom.clamp(0.6, 1.6);
        let dim = focus_i.is_some_and(|f| {
            f != i
                && !edges
                    .iter()
                    .any(|e| (e.a == f && e.b == i) || (e.b == f && e.a == i))
        });
        let a = if dim { 0.25 } else { 1.0 };
        let tagged = !n.tags.is_empty();
        let ring = if n.is_self {
            ACCENT
        } else if tagged {
            PURPLE
        } else {
            Color32::from_rgb(0x55, 0x5f, 0x70)
        };
        let fill = if n.is_self {
            Color32::from_rgb(0x16, 0x2a, 0x4d)
        } else {
            CARD_HOVER
        };
        if selected.as_deref() == Some(&n.stable_id) || st.hovered.as_deref() == Some(&n.stable_id)
        {
            painter.circle_filled(p, r + 9.0, ring.gamma_multiply(0.18 * a));
        }
        if tagged {
            let rr = Rect::from_center_size(p, Vec2::splat(r * 1.8));
            painter.rect_filled(rr, 8.0, fill.gamma_multiply(a));
            painter.rect_stroke(
                rr,
                8.0,
                Stroke::new(2.0, ring.gamma_multiply(a)),
                egui::StrokeKind::Inside,
            );
        } else {
            painter.circle_filled(p, r, fill.gamma_multiply(a));
            painter.circle_stroke(p, r, Stroke::new(2.0, ring.gamma_multiply(a)));
        }
        painter.text(
            p,
            Align2::CENTER_CENTER,
            os_icon(&n.os),
            FontId::proportional(r * 0.8),
            TEXT.gamma_multiply(a),
        );
        // Online dot.
        let dot = p + vec2(r * 0.72, -r * 0.72);
        painter.circle_filled(dot, 5.0, BG);
        painter.circle_filled(
            dot,
            3.6,
            if n.online { GREEN } else { FAINT }.gamma_multiply(a),
        );
        let label = if n.is_self {
            format!("{} (you)", n.name)
        } else {
            n.name.clone()
        };
        let g = painter.layout_no_wrap(label, FontId::proportional(13.0), TEXT.gamma_multiply(a));
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
                FontId::proportional(11.0),
                PURPLE.gamma_multiply(a),
            );
        }
    }

    legend(&painter, rect);

    // Hover card for the pair under focus.
    if let (Some(h), Some(p)) = (hit, pointer) {
        let n = nodes[h];
        let mut lines = vec![];
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
            lines.push(format!("{sym}  {}", o.name));
        }
        let text = lines.join("\n");
        let galley = painter.layout_no_wrap(text, FontId::monospace(12.0), TEXT);
        let pos = p + vec2(18.0, 12.0);
        let r = Rect::from_min_size(pos, galley.size() + vec2(20.0, 16.0));
        let r = r.translate(vec2(
            (rect.right() - r.right()).min(0.0),
            (rect.bottom() - r.bottom()).min(0.0),
        ));
        painter.rect_filled(r, 8.0, CARD);
        painter.rect_stroke(r, 8.0, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
        painter.galley(r.min + vec2(10.0, 8.0), galley, TEXT);
    }

    if energy > 0.3 || edges.iter().any(|e| e.kind == Kind::OneWay) || st.dragging.is_some() {
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

fn legend(painter: &egui::Painter, rect: Rect) {
    let x = rect.left() + 18.0;
    let mut y = rect.bottom() - 98.0;
    let box_r = Rect::from_min_size(pos2(x - 10.0, y - 12.0), vec2(300.0, 100.0));
    painter.rect_filled(box_r, 10.0, PANEL.gamma_multiply(0.92));
    painter.rect_stroke(
        box_r,
        10.0,
        Stroke::new(1.0, BORDER),
        egui::StrokeKind::Inside,
    );
    let row = |y: f32, draw: &dyn Fn(Pos2, Pos2), label: &str| {
        let a = pos2(x, y);
        let b = pos2(x + 38.0, y);
        draw(a, b);
        painter.text(
            pos2(x + 50.0, y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(12.0),
            MUTED,
        );
    };
    row(
        y,
        &|a, b| {
            painter.line_segment(
                [a, b],
                Stroke::new(1.6, Color32::from_rgb(0x4a, 0x55, 0x68)),
            );
        },
        "both directions",
    );
    y += 22.0;
    row(
        y,
        &|a, b| {
            painter.line_segment([a, b], Stroke::new(2.0, AMBER));
            arrow(painter, b, vec2(1.0, 0.0), AMBER, 9.0);
            painter.line_segment(
                [a + vec2(5.0, -5.0), a + vec2(5.0, 5.0)],
                Stroke::new(2.0, RED),
            );
        },
        "one way: arrow = allowed, red bar = blocked",
    );
    y += 22.0;
    row(
        y,
        &|a, b| {
            painter.extend(Shape::dashed_line(
                &[a, b],
                Stroke::new(1.4, MUTED),
                6.0,
                4.0,
            ));
        },
        "inferred from policy pattern",
    );
    y += 22.0;
    let c = pos2(x + 8.0, y);
    painter.rect_filled(
        Rect::from_center_size(c, Vec2::splat(14.0)),
        3.0,
        CARD_HOVER,
    );
    painter.rect_stroke(
        Rect::from_center_size(c, Vec2::splat(14.0)),
        3.0,
        Stroke::new(1.5, PURPLE),
        egui::StrokeKind::Inside,
    );
    painter.circle_stroke(pos2(x + 30.0, y), 7.0, Stroke::new(1.5, ACCENT));
    painter.text(
        pos2(x + 50.0, y),
        Align2::LEFT_CENTER,
        "tagged device, this device",
        FontId::proportional(12.0),
        MUTED,
    );
}
