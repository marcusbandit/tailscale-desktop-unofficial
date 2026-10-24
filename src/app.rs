use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, RichText, Sense,
    Stroke, Ui, UiBuilder, vec2,
};
use serde_json::json;

use crate::access::{self, Access, Probes, Source};
use crate::graph::{self, GraphState};
use crate::theme::{icon, *};
use crate::ts::{self, Node, Pong, Snapshot};

enum Msg {
    Snapshot(Result<Snapshot, String>),
    Probes(Probes),
    Ping(String, Result<Pong, String>),
    PingDone(String),
    Done(String, Result<String, String>),
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Devices,
    Graph,
    Taildrop,
}

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Online,
    Tagged,
}

struct Toast {
    text: String,
    error: bool,
    at: Instant,
}

#[derive(Default)]
struct PingLog {
    running: bool,
    results: Vec<Result<Pong, String>>,
}

pub struct App {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    wake: Sender<bool>,
    snap: Option<Snapshot>,
    error: Option<String>,
    probes: Probes,
    matrix: Option<access::Matrix>,
    view: View,
    selected: Option<String>,
    search: String,
    filter: Filter,
    toasts: Vec<Toast>,
    pings: HashMap<String, PingLog>,
    busy: HashSet<String>,
    graph: GraphState,
    shot: Option<crate::shot::Shot>,
    pending_ping: Option<Node>,
    failures: u32,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install(&cc.egui_ctx);
        let (tx, rx) = mpsc::channel();
        let (wake, wake_rx) = mpsc::channel();
        spawn_worker(tx.clone(), wake_rx, cc.egui_ctx.clone());
        Self {
            tx,
            rx,
            wake,
            snap: None,
            error: None,
            probes: Probes::new(),
            matrix: None,
            view: View::Devices,
            selected: None,
            search: String::new(),
            filter: Filter::All,
            toasts: Vec::new(),
            pings: HashMap::new(),
            busy: HashSet::new(),
            graph: GraphState::default(),
            shot: crate::shot::Shot::from_env(),
            pending_ping: None,
            failures: 0,
        }
        .with_env()
    }

    /// Test hooks: start on a view and with a device selected.
    fn with_env(mut self) -> Self {
        match std::env::var("TSDU_VIEW").as_deref() {
            Ok("graph") => self.view = View::Graph,
            Ok("taildrop") => self.view = View::Taildrop,
            _ => {}
        }
        self
    }

    fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toasts.push(Toast {
            text: text.into(),
            error,
            at: Instant::now(),
        });
    }

    fn copy(&mut self, ctx: &egui::Context, text: &str) {
        ctx.copy_text(text.to_owned());
        self.toast(format!("Copied {text}"), false);
    }

    /// Runs `f` off the UI thread; its message becomes a toast and the state is re-read.
    fn act(
        &mut self,
        ctx: &egui::Context,
        key: &str,
        f: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) {
        if !self.busy.insert(key.to_owned()) {
            return;
        }
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let key = key.to_owned();
        thread::spawn(move || {
            let r = f();
            tx.send(Msg::Done(key, r)).ok();
            ctx.request_repaint();
        });
    }

    fn ping(&mut self, ctx: &egui::Context, node: &Node) {
        let log = self.pings.entry(node.stable_id.clone()).or_default();
        if log.running {
            return;
        }
        log.running = true;
        log.results.clear();
        let (tx, ctx, id, ip) = (
            self.tx.clone(),
            ctx.clone(),
            node.stable_id.clone(),
            node.ipv4().to_owned(),
        );
        thread::spawn(move || {
            for i in 0..5 {
                if i > 0 {
                    thread::sleep(Duration::from_millis(350));
                }
                tx.send(Msg::Ping(id.clone(), ts::ping(&ip, false))).ok();
                ctx.request_repaint();
            }
            tx.send(Msg::PingDone(id)).ok();
            ctx.request_repaint();
        });
    }

    fn drain(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Snapshot(Ok(s)) => {
                    self.failures = 0;
                    if self.selected.is_none() && self.snap.is_none() {
                        if let Ok(name) = std::env::var("TSDU_SELECT") {
                            self.selected = s
                                .all()
                                .find(|n| n.name == name)
                                .map(|n| n.stable_id.clone());
                            if let Some(n) = s.all().find(|n| n.name == name).cloned() {
                                if std::env::var("TSDU_PING").is_ok() {
                                    self.pending_ping = Some(n);
                                }
                            }
                        }
                    }
                    self.snap = Some(s);
                    self.error = None;
                    self.rebuild();
                }
                Msg::Snapshot(Err(e)) => {
                    self.error = Some(e);
                    // Two misses in a row: tailscaled is gone, stop showing old state.
                    self.failures += 1;
                    if self.failures >= 2 {
                        self.snap = None;
                        self.matrix = None;
                    }
                }
                Msg::Probes(p) => {
                    // A full round replaces the last, so peers that went offline drop out.
                    self.probes = p;
                    self.rebuild();
                }
                Msg::Ping(id, r) => self.pings.entry(id).or_default().results.push(r),
                Msg::PingDone(id) => self.pings.entry(id).or_default().running = false,
                Msg::Done(key, r) => {
                    self.busy.remove(&key);
                    match r {
                        Ok(t) if !t.is_empty() => self.toast(t, false),
                        Ok(_) => {}
                        Err(e) => self.toast(e, true),
                    }
                    self.wake.send(false).ok();
                }
            }
        }
    }

    fn rebuild(&mut self) {
        self.matrix = self.snap.as_ref().map(|s| access::build(s, &self.probes));
    }
}

fn spawn_worker(tx: Sender<Msg>, wake: Receiver<bool>, ctx: egui::Context) {
    thread::spawn(move || {
        let long_ago = Instant::now() - Duration::from_secs(3600);
        let mut last_filter_try = long_ago;
        // Last packet filter read and when; older than a minute counts as unknown.
        let mut filter: Option<(Vec<ts::FilterRule>, Instant)> = None;
        let mut last_probe = long_ago;
        let mut force_probe = false;
        let probing = Arc::new(AtomicBool::new(false));
        loop {
            if last_filter_try.elapsed() > Duration::from_secs(10) {
                last_filter_try = Instant::now();
                if let Ok(f) = ts::fetch_filter() {
                    filter = Some((f, Instant::now()));
                }
            }
            let mut snap = ts::fetch_snapshot();
            if let Ok(s) = &mut snap {
                s.filter = filter
                    .as_ref()
                    .filter(|(_, at)| at.elapsed() < Duration::from_secs(60))
                    .map(|(f, _)| f.clone());
                let due = force_probe || last_probe.elapsed() > Duration::from_secs(60);
                if s.running() && due && !probing.swap(true, Ordering::SeqCst) {
                    last_probe = Instant::now();
                    force_probe = false;
                    let targets: Vec<(String, String)> = s
                        .peers
                        .iter()
                        .filter(|p| p.online)
                        .map(|p| (p.stable_id.clone(), p.ipv4().to_owned()))
                        .collect();
                    let (tx2, ctx2, probing2) = (tx.clone(), ctx.clone(), probing.clone());
                    thread::spawn(move || {
                        // Eight at a time; a second try before calling a peer closed.
                        let mut results = Probes::new();
                        for chunk in targets.chunks(8) {
                            let part: Vec<(String, bool)> = thread::scope(|sc| {
                                let hs: Vec<_> = chunk
                                    .iter()
                                    .map(|(id, ip)| {
                                        sc.spawn(move || {
                                            (
                                                id.clone(),
                                                ts::ping(ip, true).is_ok()
                                                    || ts::ping(ip, true).is_ok(),
                                            )
                                        })
                                    })
                                    .collect();
                                hs.into_iter().filter_map(|h| h.join().ok()).collect()
                            });
                            results.extend(part);
                        }
                        tx2.send(Msg::Probes(results)).ok();
                        probing2.store(false, Ordering::SeqCst);
                        ctx2.request_repaint();
                    });
                }
            }
            if tx.send(Msg::Snapshot(snap)).is_err() {
                return;
            }
            ctx.request_repaint();
            match wake.recv_timeout(Duration::from_secs(2)) {
                Ok(probe) => {
                    force_probe |= probe;
                    last_filter_try = long_ago;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return,
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Small widgets

fn toggle(ui: &mut Ui, on: bool, big: bool) -> egui::Response {
    let size = if big {
        vec2(52.0, 28.0)
    } else {
        vec2(38.0, 21.0)
    };
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id, on, 0.15);
    let bg = lerp_color(
        Color32::from_rgb(0x2b, 0x31, 0x3c),
        if big { GREEN } else { ACCENT },
        t,
    );
    let p = ui.painter();
    p.rect_filled(rect, rect.height() / 2.0, bg);
    let r = rect.height() / 2.0 - 3.0;
    let x = egui::lerp(rect.left() + r + 3.0..=rect.right() - r - 3.0, t);
    p.circle_filled(egui::pos2(x, rect.center().y), r, Color32::WHITE);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn chip(ui: &mut Ui, text: impl Into<String>, color: Color32) {
    Frame::new()
        .fill(color.gamma_multiply(0.14))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text.into()).size(11.5).color(color));
        });
}

/// Monospace text that copies itself on click.
fn copy_label(ui: &mut Ui, text: &str, size: f32) -> bool {
    let resp = Frame::new()
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).monospace().size(size).color(TEXT));
        })
        .response
        .interact(Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(resp.rect, 6.0, Color32::WHITE.gamma_multiply(0.06));
        ui.painter().text(
            resp.rect.right_center() + vec2(6.0, 0.0),
            Align2::LEFT_CENTER,
            icon::COPY,
            FontId::proportional(11.0),
            MUTED,
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text("Click to copy").clicked()
}

fn icon_button(ui: &mut Ui, glyph: &str, tip: &str, enabled: bool) -> bool {
    let b = egui::Button::new(RichText::new(glyph).size(13.0)).min_size(vec2(30.0, 28.0));
    ui.add_enabled(enabled, b)
        .on_hover_text(tip)
        .on_disabled_hover_text(tip)
        .clicked()
}

fn primary_button(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
            .fill(ACCENT.gamma_multiply(0.85))
            .stroke(Stroke::NONE)
            .min_size(vec2(0.0, 30.0)),
    )
}

fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn section_title(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.0)
            .color(MUTED)
            .strong(),
    );
}

fn ago(t: Option<DateTime<Utc>>) -> String {
    let Some(t) = t else { return "never".into() };
    let s = (Utc::now() - t).num_seconds().max(0);
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{}m ago", s / 60),
        3600..86400 => format!("{}h ago", s / 3600),
        _ => format!("{}d ago", s / 86400),
    }
}

fn local_time(t: Option<DateTime<Utc>>) -> String {
    t.map(|t| t.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn bytes(n: u64) -> String {
    let f = n as f64;
    match n {
        0..1024 => format!("{n} B"),
        1024..1_048_576 => format!("{:.1} KB", f / 1024.0),
        1_048_576..1_073_741_824 => format!("{:.1} MB", f / 1_048_576.0),
        _ => format!("{:.2} GB", f / 1_073_741_824.0),
    }
}

fn connection(n: &Node) -> (String, Color32) {
    if n.is_self {
        return ("this device".into(), ACCENT);
    }
    if !n.online {
        return ("offline".into(), FAINT);
    }
    if !n.cur_addr.is_empty() {
        ("direct".into(), GREEN)
    } else if n.active && !n.relay.is_empty() {
        (format!("relay {}", n.relay), AMBER)
    } else {
        ("idle".into(), MUTED)
    }
}

// ---------------------------------------------------------------------------
// The window

impl eframe::App for App {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        BG.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.drain();
        let ctx = ui.ctx().clone();
        self.handle_drops(&ctx);

        egui::Panel::top("top")
            .frame(
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(18, 12))
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .show(ui, |ui| self.top_bar(ui));

        if let Some(s) = &self.snap {
            if !s.health.is_empty() {
                let health = s.health.clone();
                egui::Panel::top("health")
                    .frame(
                        Frame::new()
                            .fill(AMBER.gamma_multiply(0.10))
                            .inner_margin(Margin::symmetric(18, 8)),
                    )
                    .show(ui, |ui| {
                        for h in health {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(icon::WARN).color(AMBER));
                                ui.label(RichText::new(h).color(AMBER).size(12.5));
                            });
                        }
                    });
            }
        }

        egui::Panel::left("side")
            .exact_size(250.0)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::same(14))
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .show(ui, |ui| self.sidebar(ui));

        let show_detail = self
            .selected
            .as_ref()
            .is_some_and(|id| self.snap.as_ref().is_some_and(|s| s.node(id).is_some()));
        if show_detail {
            egui::Panel::right("detail")
                .exact_size(340.0)
                .resizable(false)
                .frame(
                    Frame::new()
                        .fill(PANEL)
                        .inner_margin(Margin::same(16))
                        .stroke(Stroke::new(1.0, BORDER)),
                )
                .show(ui, |ui| self.detail(ui));
        }

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(BG)
                    .inner_margin(if self.view == View::Graph {
                        Margin::ZERO
                    } else {
                        Margin::same(18)
                    }),
            )
            .show(ui, |ui| {
                if self.snap.is_none() {
                    self.not_connected(ui);
                    return;
                }
                if !self.snap.as_ref().unwrap().running() {
                    self.stopped(ui);
                    return;
                }
                match self.view {
                    View::Devices => self.devices(ui),
                    View::Graph => {
                        let (snap, m) =
                            (self.snap.as_ref().unwrap(), self.matrix.as_ref().unwrap());
                        graph::show(ui, &mut self.graph, snap, m, &mut self.selected);
                    }
                    View::Taildrop => self.taildrop(ui),
                }
            });

        self.draw_toasts(&ctx);
        if let Some(n) = self.pending_ping.take() {
            self.ping(&ctx, &n);
        }
        if let Some(shot) = &mut self.shot {
            shot.tick(&ctx);
        }
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}

impl App {
    fn top_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Tailscale").heading());
            if let Some(s) = &self.snap {
                ui.add_space(4.0);
                ui.label(RichText::new(&s.tailnet).color(MUTED));
                ui.add_space(6.0);
                chip(ui, format!("v{}", s.version), MUTED);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, icon::REFRESH, "Refresh now and re-probe access", true) {
                    self.wake.send(true).ok();
                }
                ui.add_space(10.0);
                match self.snap.clone() {
                    Some(s) => {
                        let on = s.prefs.want_running && s.running();
                        let label = match s.backend_state.as_str() {
                            "Running" => "Connected",
                            "Stopped" => "Off",
                            "NeedsLogin" => "Logged out",
                            "Starting" => "Starting",
                            other => other,
                        }
                        .to_owned();
                        let busy = self.busy.contains("power");
                        if toggle(ui, on, true)
                            .on_hover_text("Turn Tailscale on or off")
                            .clicked()
                            && !busy
                        {
                            self.act(&ctx, "power", move || {
                                ts::set_running(!on).map(|_| {
                                    if on {
                                        "Tailscale is off".into()
                                    } else {
                                        "Tailscale is on".into()
                                    }
                                })
                            });
                        }
                        if busy {
                            ui.add(egui::Spinner::new());
                        }
                        ui.label(RichText::new(label).strong().color(if on {
                            GREEN
                        } else {
                            MUTED
                        }));
                    }
                    None => {
                        if primary_button(ui, &format!("{}   Start tailscaled", icon::POWER))
                            .clicked()
                        {
                            self.act(&ctx, "daemon", || {
                                ts::start_daemon().map(|_| "tailscaled started".into())
                            });
                        }
                    }
                }
            });
        });
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let online = self
            .snap
            .as_ref()
            .map(|s| s.peers.iter().filter(|p| p.online).count())
            .unwrap_or(0);
        let total = self.snap.as_ref().map(|s| s.peers.len()).unwrap_or(0);
        let waiting = self
            .snap
            .as_ref()
            .map(|s| s.waiting_files.len())
            .unwrap_or(0);
        for (v, glyph, name, badge) in [
            (
                View::Devices,
                icon::LIST,
                "Devices",
                format!("{online}/{total}"),
            ),
            (View::Graph, icon::GRAPH, "Access graph", String::new()),
            (
                View::Taildrop,
                icon::INBOX,
                "Taildrop",
                if waiting > 0 {
                    waiting.to_string()
                } else {
                    String::new()
                },
            ),
        ] {
            let sel = self.view == v;
            let (rect, resp) =
                ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
            let fill = if sel {
                CARD_SELECTED
            } else if resp.hovered() {
                CARD_HOVER
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, 8.0, fill);
            let fg = if sel { TEXT } else { MUTED };
            ui.painter().text(
                rect.left_center() + vec2(14.0, 0.0),
                Align2::LEFT_CENTER,
                glyph,
                FontId::proportional(14.0),
                if sel { ACCENT } else { fg },
            );
            ui.painter().text(
                rect.left_center() + vec2(40.0, 0.0),
                Align2::LEFT_CENTER,
                name,
                FontId::proportional(14.0),
                fg,
            );
            if !badge.is_empty() {
                ui.painter().text(
                    rect.right_center() - vec2(12.0, 0.0),
                    Align2::RIGHT_CENTER,
                    badge,
                    FontId::proportional(12.0),
                    MUTED,
                );
            }
            if resp.clicked() {
                self.view = v;
            }
            if resp.hovered() {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }

        let Some(s) = self.snap.clone() else { return };
        ui.add_space(18.0);
        section_title(ui, "Routing");
        ui.add_space(4.0);

        // Exit node picker.
        let exits: Vec<&Node> = s.peers.iter().filter(|p| p.exit_node_option).collect();
        let current = s
            .peers
            .iter()
            .find(|p| p.stable_id == s.prefs.exit_node_id || p.exit_node_active);
        ui.label(RichText::new(format!("{}   Exit node", icon::GLOBE)).color(TEXT));
        let mut choice: Option<Option<String>> = None;
        egui::ComboBox::from_id_salt("exit")
            .width(ui.available_width())
            .selected_text(
                current
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "None".into()),
            )
            .show_ui(ui, |ui| {
                if ui.selectable_label(current.is_none(), "None").clicked() {
                    choice = Some(None);
                }
                for e in &exits {
                    let label = if e.online {
                        e.name.clone()
                    } else {
                        format!("{} (offline)", e.name)
                    };
                    if ui
                        .selectable_label(
                            current.is_some_and(|c| c.stable_id == e.stable_id),
                            label,
                        )
                        .clicked()
                    {
                        choice = Some(Some(e.stable_id.clone()));
                    }
                }
                if exits.is_empty() {
                    ui.label(RichText::new("No device offers an exit node").color(MUTED));
                }
            });
        if let Some(c) = choice {
            self.act(&ctx, "exit", move || {
                ts::set_exit_node(c.as_deref()).map(|_| {
                    if c.is_some() {
                        "Exit node set".into()
                    } else {
                        "Exit node off".into()
                    }
                })
            });
        }

        let pref_row = |ui: &mut Ui, label: &str, hint: &str, on: bool| -> bool {
            let mut clicked = false;
            ui.horizontal(|ui| {
                ui.label(label).on_hover_text(hint);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    clicked = toggle(ui, on, false).on_hover_text(hint).clicked();
                });
            });
            clicked
        };
        ui.add_space(6.0);
        if current.is_some()
            && pref_row(
                ui,
                "Allow LAN access",
                "Keep reaching your local network while using the exit node",
                s.prefs.exit_node_allow_lan_access,
            )
        {
            let v = !s.prefs.exit_node_allow_lan_access;
            self.act(&ctx, "lan", move || {
                ts::patch_prefs(
                    json!({"ExitNodeAllowLANAccess": v, "ExitNodeAllowLANAccessSet": true}),
                )
                .map(|_| String::new())
            });
        }
        if pref_row(
            ui,
            "Use subnet routes",
            "Accept routes that other devices advertise",
            s.prefs.route_all,
        ) {
            let v = !s.prefs.route_all;
            self.act(&ctx, "routes", move || {
                ts::patch_prefs(json!({"RouteAll": v, "RouteAllSet": true})).map(|_| String::new())
            });
        }
        if pref_row(
            ui,
            "Offer as exit node",
            "Let other devices route their internet through this one",
            s.prefs.advertises_exit(),
        ) {
            let (prefs, v) = (s.prefs.clone(), !s.prefs.advertises_exit());
            self.act(&ctx, "adv", move || {
                ts::set_advertise_exit(&prefs, v).map(|_| String::new())
            });
        }
        if pref_row(
            ui,
            "Shields up",
            "Block every incoming connection to this device",
            s.prefs.shields_up,
        ) {
            let v = !s.prefs.shields_up;
            self.act(&ctx, "shields", move || {
                ts::patch_prefs(json!({"ShieldsUp": v, "ShieldsUpSet": true}))
                    .map(|_| String::new())
            });
        }

        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            let me = s.me.clone();
            ui.label(
                RichText::new(format!(
                    "tailscaled {}  ·  operator {}",
                    s.version,
                    if s.prefs.operator_user.is_empty() {
                        "root"
                    } else {
                        &s.prefs.operator_user
                    }
                ))
                .size(11.0)
                .color(FAINT),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if !me.ipv4().is_empty() && copy_label(ui, me.ipv4(), 12.5) {
                    self.copy(&ctx, me.ipv4());
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(os_icon(&me.os)).color(ACCENT));
                ui.label(RichText::new(&me.name).strong());
            });
            section_title(ui, "This device");
        });
    }

    fn not_connected(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.3);
            ui.label(RichText::new(icon::POWER).size(42.0).color(FAINT));
            ui.add_space(8.0);
            ui.label(RichText::new("Can't reach tailscaled").heading());
            if let Some(e) = &self.error {
                ui.label(RichText::new(e).color(MUTED));
            }
            ui.add_space(12.0);
            if primary_button(ui, "Start tailscaled (asks for sudo)").clicked() {
                self.act(&ctx, "daemon", || {
                    ts::start_daemon().map(|_| "tailscaled started".into())
                });
            }
        });
    }

    fn stopped(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let state = self
            .snap
            .as_ref()
            .map(|s| s.backend_state.clone())
            .unwrap_or_default();
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.3);
            ui.label(RichText::new(icon::POWER).size(42.0).color(FAINT));
            ui.add_space(8.0);
            if state == "NeedsLogin" {
                ui.label(RichText::new("Logged out").heading());
                ui.label(
                    RichText::new("Run `tailscale login` to sign this device in.").color(MUTED),
                );
            } else {
                ui.label(RichText::new("Tailscale is off").heading());
                ui.label(
                    RichText::new("This device is not on your tailnet right now.").color(MUTED),
                );
                ui.add_space(12.0);
                if primary_button(ui, "Turn on").clicked() {
                    self.act(&ctx, "power", || {
                        ts::set_running(true).map(|_| "Tailscale is on".into())
                    });
                }
            }
        });
    }

    fn devices(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let s = self.snap.clone().unwrap();
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text(format!("{}   Search name, IP, tag, user", icon::SEARCH))
                    .desired_width(280.0)
                    .margin(Margin::symmetric(10, 7)),
            );
            ui.add_space(8.0);
            for (f, label) in [
                (Filter::All, "All"),
                (Filter::Online, "Online"),
                (Filter::Tagged, "Tagged"),
            ] {
                if ui.selectable_label(self.filter == f, label).clicked() {
                    self.filter = f;
                }
            }
        });
        ui.add_space(12.0);

        let q = self.search.to_lowercase();
        let matches = |n: &Node| {
            let f = match self.filter {
                Filter::All => true,
                Filter::Online => n.online,
                Filter::Tagged => !n.tags.is_empty(),
            };
            f && (q.is_empty()
                || n.name.to_lowercase().contains(&q)
                || n.hostname.to_lowercase().contains(&q)
                || n.ips.iter().any(|i| i.contains(&q))
                || n.tags.iter().any(|t| t.contains(&q))
                || n.user_login.to_lowercase().contains(&q)
                || n.os.to_lowercase().contains(&q))
        };
        let mut nodes: Vec<&Node> = s.all().filter(|n| matches(n)).collect();
        nodes.sort_by_key(|n| (!n.is_self, !n.online, n.name.to_lowercase()));
        let (on, off): (Vec<&Node>, Vec<&Node>) = nodes.into_iter().partition(|n| n.online);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (title, list) in [("Online", on), ("Offline", off)] {
                    if list.is_empty() {
                        continue;
                    }
                    section_title(ui, &format!("{title}  ·  {}", list.len()));
                    ui.add_space(6.0);
                    for n in list {
                        self.device_row(ui, &ctx, &s, n);
                        ui.add_space(6.0);
                    }
                    ui.add_space(12.0);
                }
            });
    }

    fn device_row(&mut self, ui: &mut Ui, ctx: &egui::Context, s: &Snapshot, n: &Node) {
        let h = 66.0;
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
        let selected = self.selected.as_deref() == Some(&n.stable_id);
        let fill = if selected {
            CARD_SELECTED
        } else if resp.hovered() {
            CARD_HOVER
        } else {
            CARD
        };
        ui.painter().rect_filled(rect, 12.0, fill);
        ui.painter().rect_stroke(
            rect,
            12.0,
            Stroke::new(
                1.0,
                if selected {
                    ACCENT.gamma_multiply(0.6)
                } else {
                    BORDER
                },
            ),
            egui::StrokeKind::Inside,
        );
        if resp.clicked() {
            self.selected = if selected {
                None
            } else {
                Some(n.stable_id.clone())
            };
        }

        let inner = rect.shrink2(vec2(16.0, 10.0));
        let p = ui.painter().clone();
        p.circle_filled(
            egui::pos2(inner.left() + 5.0, inner.center().y),
            4.5,
            if n.online { GREEN } else { FAINT },
        );
        p.text(
            egui::pos2(inner.left() + 28.0, inner.center().y),
            Align2::CENTER_CENTER,
            os_icon(&n.os),
            FontId::proportional(18.0),
            if n.online { TEXT } else { MUTED },
        );

        // Fixed columns so every row lines up.
        // Columns drop out from the right as the list gets narrow.
        let avail = inner.width() - 48.0;
        let acc_w = if avail >= 720.0 { 84.0 } else { 0.0 };
        let act_w = if avail >= 640.0 { 130.0 } else { 74.0 };
        let (ip_w, conn_w) = (155.0, 115.0);
        let name_w = (avail - ip_w - conn_w - acc_w - act_w).clamp(140.0, 420.0);
        let mut x = inner.left() + 48.0;
        let mut col = |w: f32| {
            let r = egui::Rect::from_min_max(
                egui::pos2(x, inner.top()),
                egui::pos2(x + w, inner.bottom()),
            );
            x += w;
            r
        };
        let (name_r, ip_r, conn_r, acc_r) = (col(name_w), col(ip_w), col(conn_w), col(acc_w));
        let act_r = egui::Rect::from_min_max(egui::pos2(x, inner.top()), inner.max);
        let ui = &mut ui.new_child(UiBuilder::new().max_rect(rect));
        let centered = |r: egui::Rect| {
            UiBuilder::new()
                .max_rect(r)
                .layout(Layout::top_down(Align::Min).with_main_align(Align::Center))
        };

        ui.scope_builder(centered(name_r), |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(&n.name).strong().size(14.5));
                if n.is_self {
                    chip(ui, "you", ACCENT);
                }
                if n.exit_node_active {
                    chip(ui, "exit node in use", GREEN);
                } else if n.exit_node_option {
                    chip(ui, "exit node", MUTED);
                }
            });
            ui.horizontal(|ui| {
                if n.tags.is_empty() {
                    ui.label(RichText::new(&n.user_login).size(12.0).color(MUTED));
                } else {
                    for t in &n.tags {
                        chip(ui, format!("{} {t}", icon::TAG), PURPLE);
                    }
                }
            });
        });

        ui.scope_builder(
            UiBuilder::new()
                .max_rect(ip_r)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                let ip = n.ipv4().to_owned();
                if copy_label(ui, &ip, 13.0) {
                    self.copy(ctx, &ip);
                }
            },
        );

        ui.scope_builder(centered(conn_r), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let (c, col) = connection(n);
            ui.label(RichText::new(c).size(12.5).color(col));
            let seen = if n.online {
                "online now".to_owned()
            } else {
                format!("seen {}", ago(n.last_seen))
            };
            ui.label(RichText::new(seen).size(12.0).color(MUTED))
                .on_hover_text(local_time(n.last_seen));
        });

        // Access to and from this device, at a glance.
        if !n.is_self && acc_w > 0.0 {
            if let Some(m) = &self.matrix {
                ui.scope_builder(
                    UiBuilder::new()
                        .max_rect(acc_r)
                        .layout(Layout::left_to_right(Align::Center)),
                    |ui| {
                        let me = &s.me.stable_id;
                        access_line(
                            ui,
                            icon::ARROW_RIGHT,
                            "You can reach it",
                            &m.get(me, &n.stable_id),
                        );
                        ui.add_space(6.0);
                        access_line(
                            ui,
                            icon::ARROW_LEFT,
                            "It can reach you",
                            &m.get(&n.stable_id, me),
                        );
                    },
                );
            }
        }

        ui.scope_builder(
            UiBuilder::new()
                .max_rect(act_r)
                .layout(Layout::right_to_left(Align::Center)),
            |ui| {
                if !n.is_self {
                    if icon_button(ui, icon::SEND, n.taildrop_reason(), n.taildrop_ok()) {
                        self.pick_and_send(ctx, n);
                    }
                    let running = self.pings.get(&n.stable_id).is_some_and(|p| p.running);
                    if icon_button(ui, icon::BOLT, "Ping 5 times", n.online && !running) {
                        self.selected = Some(n.stable_id.clone());
                        self.ping(ctx, n);
                    }
                    let last = self
                        .pings
                        .get(&n.stable_id)
                        .and_then(|p| p.results.iter().rev().find_map(|r| r.as_ref().ok()));
                    if let Some(last) = last.filter(|_| act_w > 100.0) {
                        ui.label(
                            RichText::new(format!("{:.0} ms", last.latency_ms))
                                .monospace()
                                .size(12.0)
                                .color(if last.direct { GREEN } else { AMBER }),
                        );
                    }
                }
            },
        );
    }

    fn pick_and_send(&mut self, ctx: &egui::Context, n: &Node) {
        let (ip, name) = (n.ipv4().to_owned(), n.name.clone());
        self.act(ctx, &format!("send-{}", n.stable_id), move || {
            let Some(files) = rfd::FileDialog::new()
                .set_title(format!("Send to {name}"))
                .pick_files()
            else {
                return Ok(String::new());
            };
            send_all(&files, &ip, &name)
        });
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        let target = self
            .selected
            .as_ref()
            .and_then(|id| self.snap.as_ref()?.node(id).cloned());
        match target {
            Some(n) if n.taildrop_ok() => {
                let (ip, name) = (n.ipv4().to_owned(), n.name.clone());
                self.act(ctx, &format!("send-{}", n.stable_id), move || {
                    send_all(&dropped, &ip, &name)
                });
            }
            Some(n) => self.toast(
                format!("Can't send to {}: {}", n.name, n.taildrop_reason()),
                true,
            ),
            None => self.toast("Select a device first, then drop files to send them", true),
        }
    }

    fn detail(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let s = self.snap.clone().unwrap();
        let Some(n) = self.selected.as_ref().and_then(|id| s.node(id)).cloned() else {
            return;
        };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(os_icon(&n.os)).size(22.0).color(if n.online { TEXT } else { MUTED }));
                ui.add_space(4.0);
                ui.label(RichText::new(&n.name).heading());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(ui, icon::CLOSE, "Close", true) {
                        self.selected = None;
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                chip(ui, if n.online { "online" } else { "offline" }, if n.online { GREEN } else { MUTED });
                if !n.online {
                    chip(ui, format!("last seen {}", ago(n.last_seen)), MUTED);
                }
                for t in &n.tags {
                    chip(ui, format!("{} {t}", icon::TAG), PURPLE);
                }
                if n.tags.is_empty() && !n.user_login.is_empty() {
                    chip(ui, format!("{} {}", icon::USER, n.user_login), MUTED);
                }
            });
            ui.add_space(12.0);

            card(ui, |ui| {
                section_title(ui, "Addresses");
                ui.add_space(4.0);
                let mut addrs: Vec<String> = n.ips.clone();
                if !n.dns_name.is_empty() {
                    addrs.push(n.dns_name.clone());
                }
                for a in addrs {
                    if copy_label(ui, &a, if a.len() > 26 { 11.5 } else { 13.0 }) {
                        self.copy(&ctx, &a);
                    }
                }
            });
            ui.add_space(10.0);

            if !n.is_self {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        section_title(ui, "Ping");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let running = self.pings.get(&n.stable_id).is_some_and(|p| p.running);
                            if running {
                                ui.add(egui::Spinner::new());
                            } else if ui.add_enabled(n.online, egui::Button::new(format!("{}   Ping", icon::BOLT))).clicked() {
                                self.ping(&ctx, &n);
                            }
                        });
                    });
                    match self.pings.get(&n.stable_id) {
                        Some(log) if !log.results.is_empty() => {
                            for r in &log.results {
                                match r {
                                    Ok(p) => {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(format!("{:>6.1} ms", p.latency_ms)).monospace().color(if p.direct { GREEN } else { AMBER }));
                                            ui.label(RichText::new(format!("via {}", p.via)).size(12.0).color(MUTED));
                                        });
                                    }
                                    Err(e) => {
                                        ui.label(RichText::new(e).size(12.0).color(RED));
                                    }
                                }
                            }
                            let ok: Vec<f64> = log.results.iter().filter_map(|r| r.as_ref().ok().map(|p| p.latency_ms)).collect();
                            if !ok.is_empty() && !log.running {
                                let min = ok.iter().cloned().fold(f64::MAX, f64::min);
                                let avg = ok.iter().sum::<f64>() / ok.len() as f64;
                                ui.label(RichText::new(format!("{}/{} replies, min {min:.1} ms, avg {avg:.1} ms", ok.len(), log.results.len())).size(12.0).color(TEXT));
                            }
                        }
                        _ => {
                            ui.label(RichText::new(if n.online { "Measures latency and whether the path is direct or relayed." } else { "Offline, nothing to ping." }).size(12.0).color(MUTED));
                        }
                    }
                });
                ui.add_space(10.0);

                if let Some(m) = &self.matrix {
                    card(ui, |ui| {
                        section_title(ui, "Access");
                        ui.add_space(4.0);
                        access_detail(ui, "You can reach it", &m.get(&s.me.stable_id, &n.stable_id));
                        access_detail(ui, "It can reach you", &m.get(&n.stable_id, &s.me.stable_id));
                    });
                    ui.add_space(10.0);
                }

                card(ui, |ui| {
                    section_title(ui, "Taildrop");
                    ui.add_space(4.0);
                    ui.label(RichText::new(n.taildrop_reason()).size(12.5).color(if n.taildrop_ok() { GREEN } else { MUTED }));
                    if n.taildrop_ok() {
                        ui.add_space(4.0);
                        let busy = self.busy.contains(&format!("send-{}", n.stable_id));
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!busy, egui::Button::new(format!("{}   Send files", icon::SEND))).clicked() {
                                self.pick_and_send(&ctx, &n);
                            }
                            if busy {
                                ui.add(egui::Spinner::new());
                            }
                        });
                        ui.label(RichText::new("Or drop files anywhere on this window.").size(11.5).color(FAINT));
                    }
                });
                ui.add_space(10.0);

                if n.exit_node_option {
                    card(ui, |ui| {
                        section_title(ui, "Exit node");
                        ui.add_space(4.0);
                        let using = s.prefs.exit_node_id == n.stable_id || n.exit_node_active;
                        let id = n.stable_id.clone();
                        if using {
                            ui.label(RichText::new("Your internet traffic goes through this device.").size(12.5).color(GREEN));
                            if ui.button("Stop using it").clicked() {
                                self.act(&ctx, "exit", || ts::set_exit_node(None).map(|_| "Exit node off".into()));
                            }
                        } else if ui.add_enabled(n.online, egui::Button::new(format!("{}   Route internet through it", icon::GLOBE))).clicked() {
                            self.act(&ctx, "exit", move || ts::set_exit_node(Some(&id)).map(|_| "Exit node set".into()));
                        }
                    });
                    ui.add_space(10.0);
                }
            }

            card(ui, |ui| {
                section_title(ui, "Details");
                ui.add_space(4.0);
                let (conn, col) = connection(&n);
                let mut rows: Vec<(&str, String, Color32)> = vec![
                    ("Connection", conn, col),
                    ("OS", n.os.clone(), TEXT),
                    ("Hostname", n.hostname.clone(), TEXT),
                    ("Owner", if n.tags.is_empty() { n.user_display.clone() } else { "tagged device".into() }, TEXT),
                ];
                if !n.cur_addr.is_empty() {
                    rows.push(("Endpoint", n.cur_addr.clone(), TEXT));
                }
                if !n.relay.is_empty() {
                    rows.push(("Home relay", n.relay.clone(), TEXT));
                }
                if !n.is_self {
                    rows.push(("Sent / received", format!("{} / {}", bytes(n.tx), bytes(n.rx)), TEXT));
                }
                if !n.primary_routes.is_empty() {
                    rows.push(("Subnet routes", n.primary_routes.join(", "), TEXT));
                }
                rows.push(("Added", local_time(n.created), TEXT));
                rows.push(("Key expires", local_time(n.key_expiry), TEXT));
                if n.is_self {
                    rows.push(("Tailscale", s.version.clone(), TEXT));
                }
                egui::Grid::new("details").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                    for (k, v, c) in rows {
                        ui.label(RichText::new(k).size(12.5).color(MUTED));
                        ui.label(RichText::new(v).size(12.5).color(c));
                        ui.end_row();
                    }
                });
            });
        });
    }

    fn taildrop(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let s = self.snap.clone().unwrap();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}   Incoming files", icon::INBOX)).strong().size(15.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let busy = self.busy.contains("receive");
                        if busy {
                            ui.add(egui::Spinner::new());
                        }
                        if ui.add_enabled(!s.waiting_files.is_empty() && !busy, egui::Button::new(format!("{}   Save all to Downloads", icon::DOWNLOAD))).clicked() {
                            self.act(&ctx, "receive", || ts::receive_files().map(|d| format!("Saved to {}", d.display())));
                        }
                    });
                });
                ui.add_space(6.0);
                if s.waiting_files.is_empty() {
                    ui.label(RichText::new("Nothing waiting. Files other devices send to this one show up here.").color(MUTED));
                }
                for f in &s.waiting_files {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&f.name).monospace());
                        ui.label(RichText::new(bytes(f.size)).size(12.0).color(MUTED));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if icon_button(ui, icon::TRASH, "Discard", true) {
                                let name = f.name.clone();
                                self.act(&ctx, &format!("del-{name}"), move || ts::delete_waiting_file(&name).map(|_| format!("Discarded {name}")));
                            }
                        });
                    });
                }
            });
            ui.add_space(14.0);
            section_title(ui, "Send to");
            ui.add_space(6.0);
            let mut peers: Vec<&Node> = s.peers.iter().collect();
            peers.sort_by_key(|p| (!p.taildrop_ok(), p.name.to_lowercase()));
            for p in peers {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(os_icon(&p.os)).size(16.0));
                        ui.label(RichText::new(&p.name).strong());
                        ui.label(RichText::new(p.taildrop_reason()).size(12.0).color(if p.taildrop_ok() { GREEN } else { MUTED }));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let busy = self.busy.contains(&format!("send-{}", p.stable_id));
                            if busy {
                                ui.add(egui::Spinner::new());
                            } else if ui.add_enabled(p.taildrop_ok(), egui::Button::new(format!("{}   Send files", icon::SEND))).clicked() {
                                self.pick_and_send(&ctx, p);
                            }
                        });
                    });
                });
                ui.add_space(6.0);
            }
        });
    }

    fn draw_toasts(&mut self, ctx: &egui::Context) {
        self.toasts
            .retain(|t| t.at.elapsed() < Duration::from_millis(3200));
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -24.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                for t in &self.toasts {
                    let fade = (3.2 - t.at.elapsed().as_secs_f32()).clamp(0.0, 0.4) / 0.4;
                    Frame::new()
                        .fill(CARD_HOVER.gamma_multiply(fade))
                        .stroke(Stroke::new(
                            1.0,
                            if t.error { RED } else { BORDER }.gamma_multiply(fade),
                        ))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::symmetric(16, 10))
                        .show(ui, |ui| {
                            let (g, c) = if t.error {
                                (icon::WARN, RED)
                            } else {
                                (icon::CHECK, GREEN)
                            };
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(g).color(c.gamma_multiply(fade)));
                                ui.label(RichText::new(&t.text).color(TEXT.gamma_multiply(fade)));
                            });
                        });
                    ui.add_space(6.0);
                }
            });
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}

fn send_all(files: &[PathBuf], ip: &str, name: &str) -> Result<String, String> {
    for f in files {
        ts::send_file(f, ip)?;
    }
    Ok(format!(
        "Sent {} file{} to {name}",
        files.len(),
        if files.len() == 1 { "" } else { "s" }
    ))
}

fn access_text(a: &Access) -> (&'static str, Color32, String) {
    let how = |s: Source| match s {
        Source::Exact => "from this device's packet filter",
        Source::Probed => "tested with a ping through the tunnel",
        Source::Inferred => "inferred from devices with the same owner and tags",
    };
    match a {
        Access::Allowed { source, ports } => {
            let extra = if ports.is_empty() {
                String::new()
            } else {
                format!("{ports}, ")
            };
            (icon::CHECK, GREEN, format!("{extra}{}", how(*source)))
        }
        Access::Denied(source) => (icon::BAN, RED, how(*source).to_owned()),
        Access::Unknown => ("?", MUTED, "not known yet".into()),
    }
}

fn access_line(ui: &mut Ui, arrow: &str, what: &str, a: &Access) {
    let (glyph, col, how) = access_text(a);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new(arrow).size(11.0).color(MUTED));
        ui.label(RichText::new(glyph).size(11.0).color(col));
    })
    .response
    .on_hover_text(format!("{what}: {how}"));
}

fn access_detail(ui: &mut Ui, what: &str, a: &Access) {
    let (glyph, col, how) = access_text(a);
    ui.horizontal(|ui| {
        ui.label(RichText::new(glyph).color(col));
        ui.label(RichText::new(what).color(TEXT));
    });
    ui.label(RichText::new(how).size(11.5).color(MUTED));
    ui.add_space(4.0);
}
