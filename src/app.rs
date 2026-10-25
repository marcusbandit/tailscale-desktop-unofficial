use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, RichText, Sense,
    Stroke, Ui, UiBuilder, vec2,
};

use crate::access::{self, Access, Probes, Source};
use crate::graph::{self, GraphState};
use crate::osinfo::{self, OsInfo};
use crate::privacy::Mask;
use crate::settings::Settings;
use crate::theme::{icon, *};
use crate::ts::{self, Node, Pong, Snapshot};

mod onboard;
mod replay;
mod settings_view;

enum Msg {
    Snapshot(Result<Snapshot, String>),
    Probes(Probes),
    Ping(String, Result<Pong, String>),
    PingDone(String),
    Done(String, Result<String, String>),
    /// This device's own Hostinfo.
    OsLocal(OsInfo),
    /// The API's view of every device; `None` when no credential is stored.
    /// Tagged with the epoch it was fetched in, so a reply that started before a
    /// disconnect or a new connect is dropped.
    OsApi(u64, Option<Result<HashMap<String, OsInfo>, String>>),
    /// A connect attempt checked its credential: (attempt, secret and devices).
    Verified(u64, Result<(String, HashMap<String, OsInfo>), String>),
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Devices,
    Graph,
    Taildrop,
    Settings,
}

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Online,
    Tagged,
}

impl Filter {
    fn admits(self, n: &Node) -> bool {
        match self {
            Filter::All => true,
            Filter::Online => n.online || n.is_self,
            Filter::Tagged => !n.tags.is_empty(),
        }
    }
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
    os_local: Option<OsInfo>,
    os_api: HashMap<String, OsInfo>,
    /// Both of the above, merged, keyed by stable node id.
    os: HashMap<String, OsInfo>,
    api: ApiState,
    os_wake: Sender<()>,
    os_epoch: Arc<AtomicU64>,
    /// First-run walkthrough; while open it takes the whole window.
    onboard: Option<Onboarding>,
    /// Bumped by every connect attempt and every cancel, so a check that comes
    /// back after the dialog closed is ignored.
    connect_attempt: u64,
    settings: Settings,
    /// How privacy mode shows names and addresses; rebuilt when either changes.
    mask: Rc<Mask>,
    /// Health warnings closed this session.
    dismissed: HashSet<String>,
    /// Test hook: open the status menu on the first frame.
    open_menu: bool,
}

#[derive(Default)]
struct ApiState {
    connected: bool,
    error: Option<String>,
}

#[derive(Default)]
struct Onboarding {
    step: usize,
    secret: String,
    error: Option<String>,
    busy: bool,
    /// Set once a credential checked out during this walkthrough.
    connected: bool,
    replay: replay::Replay,
}

impl Onboarding {
    fn at(step: usize) -> Self {
        Self {
            step,
            ..Default::default()
        }
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install(&cc.egui_ctx);
        let (tx, rx) = mpsc::channel();
        let (wake, wake_rx) = mpsc::channel();
        spawn_worker(tx.clone(), wake_rx, cc.egui_ctx.clone());
        let (os_wake, os_wake_rx) = mpsc::channel();
        let os_epoch = Arc::new(AtomicU64::new(0));
        spawn_os_worker(
            tx.clone(),
            os_wake_rx,
            os_epoch.clone(),
            cc.egui_ctx.clone(),
        );
        // Cached versions only count while a credential is stored.
        let (api, os_api) = match osinfo::load_secret() {
            Ok(Some(_)) => (
                ApiState {
                    connected: true,
                    error: None,
                },
                osinfo::load_cache(),
            ),
            Ok(None) => {
                osinfo::clear_cache();
                (ApiState::default(), HashMap::new())
            }
            Err(e) => (
                ApiState {
                    connected: true,
                    error: Some(e),
                },
                osinfo::load_cache(),
            ),
        };
        let settings = Settings::load();
        Self {
            mask: Rc::new(Mask::new(settings.privacy, settings.privacy_names, None)),
            settings,
            dismissed: HashSet::new(),
            open_menu: std::env::var("TSDU_MENU").is_ok(),
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
            os_local: None,
            os: os_api.clone(),
            api,
            os_api,
            os_wake,
            os_epoch,
            connect_attempt: 0,
            onboard: onboard::first_step().map(Onboarding::at),
        }
        .with_env()
    }

    /// Test hooks: start on a view and with a device selected.
    fn with_env(mut self) -> Self {
        match std::env::var("TSDU_VIEW").as_deref() {
            Ok("graph") => self.view = View::Graph,
            Ok("taildrop") => self.view = View::Taildrop,
            Ok("settings") => self.view = View::Settings,
            _ => {}
        }
        self
    }

    fn remask(&mut self) {
        let s = &self.settings;
        self.mask = Rc::new(Mask::new(s.privacy, s.privacy_names, self.snap.as_ref()));
    }

    /// Changes a setting, saves it and redraws with it.
    fn set(&mut self, f: impl FnOnce(&mut Settings)) {
        f(&mut self.settings);
        if let Err(e) = self.settings.save() {
            let e = self.mask.text(&e);
            self.toast(format!("Could not save settings: {e}"), true);
        }
        self.remask();
    }

    fn toggle_privacy(&mut self) {
        self.set(|s| s.privacy = !s.privacy);
        // Toasts are masked when made, so the ones from before go.
        if self.settings.privacy {
            self.toasts.clear();
        }
        let text = if self.settings.privacy {
            "Privacy mode is on"
        } else {
            "Privacy mode is off"
        };
        self.toast(text, false);
    }

    fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toasts.push(Toast {
            text: text.into(),
            error,
            at: Instant::now(),
        });
    }

    /// Copies the real value; in privacy mode the toast doesn't repeat it.
    fn copy(&mut self, ctx: &egui::Context, text: &str) {
        ctx.copy_text(text.to_owned());
        let shown = if self.mask.on {
            "Copied".to_owned()
        } else {
            format!("Copied {text}")
        };
        self.toast(shown, false);
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
                    let first = self.snap.is_none();
                    self.snap = Some(s);
                    self.error = None;
                    self.rebuild();
                    self.remask();
                    if first {
                        self.merge_os();
                    }
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
                    // tailscaled only re-checks forwarding on its next reconfig,
                    // so the fixed warning is closed here.
                    if key == "forwarding" && r.is_ok() {
                        let health = self.snap.as_ref().map(|s| s.health.clone());
                        self.dismissed.extend(
                            health
                                .unwrap_or_default()
                                .into_iter()
                                .filter(|h| ts::is_forwarding_warning(h)),
                        );
                    }
                    match r {
                        Ok(t) if !t.is_empty() => self.toast(self.mask.text(&t), false),
                        Ok(_) => {}
                        // Errors come from the CLI or the system, in their own words.
                        Err(e) => self.toast(self.mask.external(&e), true),
                    }
                    self.wake.send(false).ok();
                }
                Msg::OsLocal(info) => {
                    self.os_local = Some(info);
                    self.merge_os();
                }
                Msg::OsApi(epoch, _) if epoch != self.os_epoch.load(Ordering::SeqCst) => {}
                Msg::OsApi(_, None) => {
                    osinfo::clear_cache();
                    self.api = ApiState::default();
                    self.os_api.clear();
                    self.merge_os();
                }
                Msg::OsApi(_, Some(Ok(map))) => self.apply_os(map),
                Msg::OsApi(_, Some(Err(e))) => {
                    // Keep showing the last good versions; say what went wrong.
                    self.api.connected = true;
                    self.api.error = Some(e);
                }
                Msg::Verified(attempt, _) if attempt != self.connect_attempt => {}
                Msg::Verified(_, r) => {
                    let r = r.and_then(|(secret, map)| {
                        osinfo::store_secret(&secret)?;
                        Ok(map)
                    });
                    match r {
                        Ok(map) => {
                            // Anything the worker had in flight predates this credential.
                            self.os_epoch.fetch_add(1, Ordering::SeqCst);
                            self.apply_os(map);
                            match &mut self.onboard {
                                Some(o) => {
                                    o.busy = false;
                                    o.connected = true;
                                }
                                None => self.toast("Connected. OS versions are on.", false),
                            }
                        }
                        Err(e) => {
                            if let Some(d) = &mut self.onboard {
                                d.busy = false;
                                d.error = Some(e);
                            }
                        }
                    }
                }
            }
        }
    }

    fn apply_os(&mut self, map: HashMap<String, OsInfo>) {
        self.api = ApiState {
            connected: true,
            error: None,
        };
        osinfo::save_cache(&map);
        self.os_api = map;
        self.merge_os();
    }

    fn merge_os(&mut self) {
        let me = self.snap.as_ref().map(|s| s.me.stable_id.clone());
        let local = me.as_deref().zip(self.os_local.as_ref());
        self.os = osinfo::combine(&self.os_api, local);
    }

    /// What is known about a device's system, at least the OS family.
    fn os_of(&self, n: &Node) -> OsInfo {
        self.os
            .get(&n.stable_id)
            .cloned()
            .unwrap_or_else(|| OsInfo {
                os: n.os.clone(),
                ..Default::default()
            })
    }

    fn start_connect(&mut self, ctx: &egui::Context) {
        let Some(d) = &mut self.onboard else {
            return;
        };
        d.busy = true;
        d.error = None;
        self.connect_attempt += 1;
        let attempt = self.connect_attempt;
        let secret = d.secret.trim().to_owned();
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        thread::spawn(move || {
            // Prove the credential works; the UI keeps it only if the dialog is
            // still waiting for this attempt.
            let r = osinfo::Api::new(secret.clone())
                .fetch()
                .map(|map| (secret, map));
            tx.send(Msg::Verified(attempt, r)).ok();
            ctx.request_repaint();
        });
    }

    fn rebuild(&mut self) {
        self.matrix = self.snap.as_ref().map(|s| access::build(s, &self.probes));
    }

    /// Leaves the walkthrough; a check still running is ignored when it returns.
    fn close_onboarding(&mut self) {
        self.onboard = None;
        self.connect_attempt += 1;
        if let Err(e) = onboard::mark_done() {
            let e = self.mask.text(&e);
            self.toast(format!("Could not save that the tour is done: {e}"), true);
        }
    }

    fn disconnect(&mut self) {
        self.os_epoch.fetch_add(1, Ordering::SeqCst);
        osinfo::clear_secret();
        osinfo::clear_cache();
        self.api = ApiState::default();
        self.os_api.clear();
        self.merge_os();
        self.os_wake.send(()).ok();
        self.toast(
            "Credential removed from the keyring. OS versions are off.",
            false,
        );
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

/// Reads this device's Hostinfo once, then asks the Tailscale API about every
/// device every 15 minutes while a credential is stored.
fn spawn_os_worker(tx: Sender<Msg>, wake: Receiver<()>, epoch: Arc<AtomicU64>, ctx: egui::Context) {
    thread::spawn(move || {
        if let Some(info) = osinfo::local() {
            tx.send(Msg::OsLocal(info)).ok();
            ctx.request_repaint();
        }
        let mut api: Option<(String, osinfo::Api)> = None;
        loop {
            let started = epoch.load(Ordering::SeqCst);
            let msg = match osinfo::load_secret() {
                Ok(None) => {
                    api = None;
                    None
                }
                Err(e) => Some(Err(e)),
                Ok(Some(secret)) => {
                    if api.as_ref().is_none_or(|(s, _)| *s != secret) {
                        api = Some((secret.clone(), osinfo::Api::new(secret)));
                    }
                    Some(api.as_mut().unwrap().1.fetch())
                }
            };
            if tx.send(Msg::OsApi(started, msg)).is_err() {
                return;
            }
            ctx.request_repaint();
            match wake.recv_timeout(Duration::from_secs(15 * 60)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
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

fn chip(ui: &mut Ui, text: impl Into<String>, color: Color32) -> egui::Response {
    Frame::new()
        .fill(color.gamma_multiply(0.14))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text.into()).size(11.5).color(color));
        })
        .response
}

/// One entry in the sidebar's navigation.
fn nav_item(ui: &mut Ui, sel: bool, glyph: &str, name: &str, badge: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    let fill = if sel {
        CARD_SELECTED
    } else if resp.hovered() {
        CARD_HOVER
    } else {
        Color32::TRANSPARENT
    };
    let p = ui.painter();
    p.rect_filled(rect, 8.0, fill);
    let fg = if sel { TEXT } else { MUTED };
    paint_icon(
        p,
        rect.left_center() + vec2(20.0, 0.0),
        glyph,
        14.0,
        if sel { ACCENT } else { fg },
    );
    p.text(
        rect.left_center() + vec2(40.0, 0.0),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(14.0),
        fg,
    );
    if !badge.is_empty() {
        p.text(
            rect.right_center() - vec2(12.0, 0.0),
            Align2::RIGHT_CENTER,
            badge,
            FontId::proportional(12.0),
            MUTED,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// A filter tab with its count beside it, dimmer than the label.
fn tab(ui: &mut Ui, sel: bool, label: &str, count: usize) -> egui::Response {
    let mut job = egui::text::LayoutJob::default();
    let style = ui.style().clone();
    RichText::new(label).color(if sel { TEXT } else { MUTED }).append_to(
        &mut job,
        &style,
        egui::FontSelection::Default,
        Align::Center,
    );
    RichText::new(format!("  {count}"))
        .color(if sel { MUTED } else { FAINT })
        .append_to(
        &mut job,
        &style,
        egui::FontSelection::Default,
        Align::Center,
    );
    ui.selectable_label(sel, job)
}

/// The header's one status control: a dot, the state in words, what the
/// traffic goes through when an exit node is on, and a chevron for its menu.
fn pill(ui: &mut Ui, dot: Color32, text: &str, sub: &str) -> egui::Response {
    let p = ui.painter().clone();
    let g = p.layout_no_wrap(text.to_owned(), FontId::proportional(13.5), TEXT);
    let s = (!sub.is_empty())
        .then(|| p.layout_no_wrap(sub.to_owned(), FontId::proportional(12.5), MUTED));
    let (gw, gh) = (g.size().x, g.size().y);
    let sw = s.as_ref().map_or(0.0, |s| s.size().x + 8.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(32.0 + gw + sw + 32.0, 32.0), Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let fill = if open || resp.hovered() { CARD_HOVER } else { CARD };
    p.rect(
        rect,
        16.0,
        fill,
        Stroke::new(1.0, BORDER),
        egui::StrokeKind::Inside,
    );
    let c = rect.left_center() + vec2(17.0, 0.0);
    p.circle_filled(c, 7.0, dot.gamma_multiply(0.22));
    p.circle_filled(c, 4.0, dot);
    let x = rect.left() + 32.0;
    p.galley(egui::pos2(x, rect.center().y - gh / 2.0), g, TEXT);
    if let Some(s) = s {
        let sh = s.size().y;
        p.galley(egui::pos2(x + gw + 8.0, rect.center().y - sh / 2.0), s, MUTED);
    }
    paint_icon(
        &p,
        rect.right_center() - vec2(17.0, 0.0),
        icon::CHEVRON_DOWN,
        10.0,
        MUTED,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// A row in the status menu.
fn menu_item(ui: &mut Ui, glyph: &str, text: &str) -> bool {
    let resp = ui.add(egui::Button::new("").min_size(vec2(ui.available_width(), 32.0)));
    let p = ui.painter();
    paint_icon(p, resp.rect.left_center() + vec2(14.0, 0.0), glyph, 13.0, MUTED);
    p.text(
        resp.rect.left_center() + vec2(32.0, 0.0),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(13.5),
        TEXT,
    );
    resp.clicked()
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

/// One numbered step in a short how-to, with room for a control under the text.
fn step(ui: &mut Ui, n: &str, text: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal_top(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 11.0, CARD_HOVER);
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            n,
            FontId::proportional(12.0),
            TEXT,
        );
        ui.vertical(|ui| {
            ui.add_space(2.0);
            ui.label(text);
            add(ui);
        });
    });
}

/// This device first, then online before offline, then by name as shown.
fn display_order(n: &Node, mask: &Mask) -> (bool, bool, String) {
    (!n.is_self, !n.online, mask.order(n))
}

/// The one surface style for grouped content.
fn card_frame(margin: i8) -> Frame {
    Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(margin))
}

fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_frame(14)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn section_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.5).color(MUTED).strong());
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

        let privacy_key =
            egui::KeyboardShortcut::new(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::P);
        if ctx.input_mut(|i| i.consume_shortcut(&privacy_key)) {
            self.toggle_privacy();
        }

        if self.onboard.is_some() {
            // Escape skips the walkthrough, the same as its Skip buttons.
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.close_onboarding();
            }
        }
        if self.onboard.is_some() {
            egui::CentralPanel::default()
                .frame(Frame::new().fill(BG))
                .show(ui, |ui| self.onboarding(ui));
            self.draw_toasts(&ctx);
            if let Some(shot) = &mut self.shot {
                shot.tick(&ctx);
            }
            ctx.request_repaint_after(Duration::from_secs(1));
            return;
        }
        self.handle_drops(&ctx);

        egui::Panel::top("top")
            .frame(
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(18, 12))
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .show(ui, |ui| self.top_bar(ui));

        let health: Vec<String> = self.snap.as_ref().map_or_else(Vec::new, |s| {
            s.health
                .iter()
                .filter(|h| !self.dismissed.contains(*h))
                .cloned()
                .collect()
        });
        if let Some(prefs) = self.snap.as_ref().map(|s| s.prefs.clone()) {
            if !health.is_empty() {
                egui::Panel::top("health")
                    .frame(
                        Frame::new()
                            .fill(AMBER.gamma_multiply(0.10))
                            .inner_margin(Margin::symmetric(18, 6)),
                    )
                    .show(ui, |ui| {
                        for h in health {
                            self.health_row(ui, &prefs, &h);
                        }
                    });
            }
        }

        egui::Panel::left("side")
            .exact_size(210.0)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::same(14))
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .show(ui, |ui| self.sidebar(ui));

        let show_detail = self.view != View::Settings
            && self
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
                // Settings work whatever state tailscaled is in.
                if self.view == View::Settings {
                    self.settings_view(ui);
                    return;
                }
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
                        let mask = self.mask.clone();
                        graph::show(ui, &mut self.graph, snap, m, &mut self.selected, &mask);
                    }
                    View::Taildrop => self.taildrop(ui),
                    View::Settings => {}
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
            let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
            paint_mark(ui.painter(), r);
            ui.add_space(4.0);
            ui.label(RichText::new("Tailscale Desktop").heading());
            chip(ui, "unofficial", MUTED);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                match self.snap.clone() {
                    Some(s) => self.status(ui, &s),
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
                ui.add_space(6.0);
                let on = self.mask.on;
                let glyph = if on { icon::EYE_SLASH } else { icon::EYE };
                let b = egui::Button::new(
                    RichText::new(glyph)
                        .size(13.0)
                        .color(if on { ACCENT } else { MUTED }),
                )
                .min_size(vec2(32.0, 32.0))
                .corner_radius(CornerRadius::same(16))
                .fill(if on { ACCENT.gamma_multiply(0.16) } else { CARD });
                let tip = if on {
                    "Privacy mode is on: addresses, accounts and owners are hidden. Ctrl+Shift+P"
                } else {
                    "Privacy mode: hide addresses, accounts and owners, for streams and screenshots. Ctrl+Shift+P"
                };
                if ui.add(b).on_hover_text(tip).clicked() {
                    self.toggle_privacy();
                }
                if on {
                    chip(ui, "Privacy on", ACCENT);
                }
            });
        });
    }

    /// The status pill and the menu behind it: the account, the on/off switch,
    /// and the things that used to crowd the header and sidebar.
    fn status(&mut self, ui: &mut Ui, s: &Snapshot) {
        let ctx = ui.ctx().clone();
        let on = s.prefs.want_running && s.running();
        let (label, dot) = match s.backend_state.as_str() {
            "Running" => ("Connected", GREEN),
            "Stopped" => ("Off", RING_OFF),
            "NeedsLogin" => ("Logged out", AMBER),
            "Starting" => ("Starting", AMBER),
            other => (other, AMBER),
        };
        let via = s
            .peers
            .iter()
            .find(|p| p.stable_id == s.prefs.exit_node_id || p.exit_node_active)
            .filter(|_| on)
            .map(|p| format!("via {}", self.mask.name(p)))
            .unwrap_or_default();
        let resp = pill(ui, dot, label, &via);
        let mut popup = egui::Popup::menu(&resp)
            .align(egui::RectAlign::BOTTOM_END)
            .gap(6.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
        if std::mem::take(&mut self.open_menu) {
            popup = popup.open_memory(Some(egui::SetOpenCommand::Bool(true)));
        }
        popup.show(|ui| {
            ui.set_width(280.0);
            Frame::new().inner_margin(Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("Tailnet").size(11.5).color(MUTED));
                ui.label(RichText::new(self.mask.who(&s.tailnet)).strong());
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!(
                        "Tailscale {}  ·  {}",
                        s.version,
                        self.mask.name(&s.me)
                    ))
                    .size(11.5)
                    .color(FAINT),
                );
            });
            ui.separator();
            Frame::new().inner_margin(Margin::symmetric(10, 2)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label("Tailscale");
                        ui.label(
                            RichText::new(if on {
                                "This device is on the tailnet"
                            } else {
                                "This device is off the tailnet"
                            })
                            .size(11.5)
                            .color(MUTED),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let busy = self.busy.contains("power");
                        if toggle(ui, on, true).clicked() && !busy {
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
                    });
                });
            });
            ui.separator();
            if menu_item(ui, icon::REFRESH, "Check access again") {
                self.wake.send(true).ok();
                self.toast("Checking who can reach whom", false);
                ui.close();
            }
            if menu_item(ui, icon::GEAR, "Settings") {
                self.view = View::Settings;
                ui.close();
            }
            if menu_item(ui, icon::HELP, "Welcome tour") {
                self.onboard = Some(Onboarding::at(0));
                ui.close();
            }
        });
    }

    /// One health warning from tailscaled, with a way to fix it when there is one.
    /// The buttons are placed first and the text wraps in what is left.
    fn health_row(&mut self, ui: &mut Ui, prefs: &ts::Prefs, h: &str) {
        let ctx = ui.ctx().clone();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let close = egui::Button::new(RichText::new(icon::CLOSE).size(12.0).color(AMBER))
                .frame(false)
                .min_size(vec2(26.0, 26.0));
            if ui.add(close).on_hover_text("Hide this warning").clicked() {
                self.dismissed.insert(h.to_owned());
            }
            if ts::is_forwarding_warning(h) {
                ui.add_space(4.0);
                let more = egui::Button::new(
                    RichText::new(format!("How to fix  {}", icon::EXTERNAL))
                        .size(12.5)
                        .color(AMBER),
                )
                .frame(false);
                if ui
                    .add(more)
                    .on_hover_text("Tailscale's guide to IP forwarding")
                    .clicked()
                {
                    ctx.open_url(egui::OpenUrl::new_tab(ts::FORWARDING_URL));
                }
                if prefs.advertises_exit() {
                    let prefs = prefs.clone();
                    if ui
                        .button(RichText::new("Stop offering exit node").size(12.5))
                        .on_hover_text("Other devices can no longer send their traffic through this one")
                        .clicked()
                    {
                        self.act(&ctx, "adv", move || {
                            ts::set_advertise_exit(&prefs, false)
                                .map(|_| "No longer offered as an exit node".into())
                        });
                    }
                }
                let busy = self.busy.contains("forwarding");
                let fix = egui::Button::new(
                    RichText::new("Turn on IP forwarding")
                        .size(12.5)
                        .color(Color32::BLACK)
                        .strong(),
                )
                .fill(AMBER)
                .stroke(Stroke::NONE);
                if ui
                    .add_enabled(!busy, fix)
                    .on_hover_text("Asks for your password, then turns forwarding on now and after every reboot")
                    .clicked()
                {
                    let prefs = prefs.clone();
                    self.act(&ctx, "forwarding", move || {
                        ts::enable_ip_forwarding(&prefs).map(|_| "IP forwarding is on".into())
                    });
                }
                if busy {
                    ui.add(egui::Spinner::new());
                }
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.label(RichText::new(icon::WARN).color(AMBER));
                ui.add(
                    egui::Label::new(RichText::new(self.mask.external(h)).color(AMBER).size(12.5))
                        .wrap(),
                );
            });
        });
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        let (online, total, waiting) = self.snap.as_ref().map_or((0, 0, 0), |s| {
            (
                s.all().filter(|n| n.online || n.is_self).count(),
                s.all().count(),
                s.waiting_files.len(),
            )
        });
        let devices = if total > 0 {
            format!("{online}/{total}")
        } else {
            String::new()
        };
        let files = if waiting > 0 {
            waiting.to_string()
        } else {
            String::new()
        };
        for (v, glyph, name, badge) in [
            (View::Devices, icon::LIST, "Devices", devices),
            (View::Graph, icon::GRAPH, "Access graph", String::new()),
            (View::Taildrop, icon::INBOX, "Taildrop", files),
        ] {
            if nav_item(ui, self.view == v, glyph, name, &badge).clicked() {
                self.view = v;
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            if nav_item(ui, self.view == View::Settings, icon::GEAR, "Settings", "").clicked() {
                self.view = View::Settings;
            }
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
                ui.label(RichText::new(self.mask.external(e)).color(MUTED));
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
                    .hint_text(format!("{}   Search, or tag:  os:  user:", icon::SEARCH))
                    .desired_width(300.0)
                    .margin(Margin::symmetric(10, 7)),
            );
            ui.add_space(8.0);
            for (f, label) in [
                (Filter::All, "All"),
                (Filter::Online, "Online"),
                (Filter::Tagged, "Tagged"),
            ] {
                let count = s.all().filter(|n| f.admits(n)).count();
                if tab(ui, self.filter == f, label, count).clicked() {
                    self.filter = f;
                }
            }
        });
        ui.add_space(14.0);

        let mut nodes: Vec<&Node> = s
            .all()
            .filter(|n| self.filter.admits(n) && self.matches(n, &self.search))
            .collect();
        // This device, then online, then offline; offline rows dim themselves.
        nodes.sort_by_key(|n| display_order(n, &self.mask));

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if nodes.is_empty() {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("No device matches").color(MUTED));
                        ui.label(
                            RichText::new("Try a name, an IP, tag:server, os:android or user:alice.")
                                .size(12.5)
                                .color(FAINT),
                        );
                    });
                    return;
                }
                card_frame(4).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let last = nodes.len() - 1;
                    for (i, n) in nodes.into_iter().enumerate() {
                        self.device_row(ui, &ctx, &s, n);
                        if i != last {
                            let r = ui.available_rect_before_wrap();
                            ui.painter().hline(
                                r.left() + 64.0..=r.right() - 12.0,
                                r.top(),
                                Stroke::new(1.0, BORDER),
                            );
                        }
                    }
                });
                ui.add_space(18.0);
            });
    }

    /// Every word must match. `tag:`, `os:`, `user:` and `is:` look in one
    /// place; a bare word looks everywhere.
    fn matches(&self, n: &Node, query: &str) -> bool {
        let os = self.os_of(n);
        let has = |hay: &str, v: &str| hay.to_lowercase().contains(v);
        query.split_whitespace().all(|word| {
            let w = word.to_lowercase();
            if let Some(v) = w.strip_prefix("tag:") {
                n.tags.iter().any(|t| has(t.trim_start_matches("tag:"), v))
            } else if let Some(v) = w.strip_prefix("os:") {
                has(&os.title(), v) || has(&n.os, v)
            } else if let Some(v) = w.strip_prefix("user:") {
                has(&n.user_login, v) || has(&n.user_display, v)
            } else if let Some(v) = w.strip_prefix("is:") {
                match v {
                    "online" => Filter::Online.admits(n),
                    "offline" => !Filter::Online.admits(n),
                    "tagged" => Filter::Tagged.admits(n),
                    "exit" => n.exit_node_option,
                    _ => false,
                }
            } else {
                has(&n.name, &w)
                    || has(&n.hostname, &w)
                    || has(&self.mask.name(n), &w)
                    || n.ips.iter().any(|i| i.contains(&w))
                    || n.tags.iter().any(|t| has(t, &w))
                    || has(&n.user_login, &w)
                    || has(&os.title(), &w)
                    || has(&n.os, &w)
            }
        })
    }

    fn device_row(&mut self, ui: &mut Ui, ctx: &egui::Context, s: &Snapshot, n: &Node) {
        let mask = self.mask.clone();
        let h = 60.0;
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
        let selected = self.selected.as_deref() == Some(&n.stable_id);
        // The pointer may be over a button inside the row; it still counts.
        let hover = ui.rect_contains_pointer(rect);
        if selected || hover {
            let fill = if selected { CARD_SELECTED } else { CARD_HOVER };
            ui.painter().rect_filled(rect, 9.0, fill);
        }
        if resp.clicked() {
            self.selected = if selected {
                None
            } else {
                Some(n.stable_id.clone())
            };
        }
        if resp.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let inner = rect.shrink2(vec2(14.0, 6.0));
        let dim = !n.online && !n.is_self;
        paint_badge(
            ui.painter(),
            egui::pos2(inner.left() + 18.0, inner.center().y),
            17.0,
            os_icon(&n.os),
            Badge::of(n),
            1.0,
        );

        // Fixed columns so every row lines up; the IP gives way first when the
        // list gets narrow.
        let avail = inner.width() - 50.0;
        let act_w = 96.0;
        let ip_w = if self.settings.show_ips && avail >= 460.0 {
            150.0
        } else {
            0.0
        };
        let name_w = (avail - ip_w - act_w).clamp(150.0, 420.0);
        let mut x = inner.left() + 50.0;
        let mut col = |w: f32| {
            let r = egui::Rect::from_min_max(
                egui::pos2(x, inner.top()),
                egui::pos2(x + w, inner.bottom()),
            );
            x += w;
            r
        };
        let (name_r, ip_r) = (col(name_w), col(ip_w));
        let act_r = egui::Rect::from_min_max(egui::pos2(x, inner.top()), inner.max);
        let ui = &mut ui.new_child(UiBuilder::new().max_rect(rect));
        let centered = |r: egui::Rect| {
            UiBuilder::new()
                .max_rect(r)
                .layout(Layout::top_down(Align::Min).with_main_align(Align::Center))
        };
        let in_row = |r: egui::Rect| {
            UiBuilder::new()
                .max_rect(r)
                .layout(Layout::left_to_right(Align::Center))
        };

        // Access only speaks up when something is blocked; open both ways is normal.
        let blocked = (!n.is_self)
            .then_some(self.matrix.as_ref())
            .flatten()
            .and_then(|m| {
                let me = &s.me.stable_id;
                let (out, inn) = (m.get(me, &n.stable_id), m.get(&n.stable_id, me));
                match (out.allowed(), inn.allowed()) {
                    (Some(false), Some(false)) => Some(("No access", RED, out)),
                    (Some(false), _) => Some(("You can't reach it", RED, out)),
                    (_, Some(false)) => Some(("Can't reach you", AMBER, inn)),
                    _ => None,
                }
            });

        // Status only when there is something to say: a relay, a ping result,
        // or when an offline device was last around. Direct is the normal case.
        let last_ping = self
            .pings
            .get(&n.stable_id)
            .and_then(|p| p.results.iter().rev().find_map(|r| r.as_ref().ok()));
        let relayed = n.cur_addr.is_empty() && n.active && !n.relay.is_empty();
        let status = if n.is_self {
            None
        } else if !n.online {
            Some((
                format!("Seen {}", ago(n.last_seen)),
                FAINT,
                format!("Last seen {}", local_time(n.last_seen)),
            ))
        } else if relayed {
            Some((
                format!("Relay {}", n.relay),
                AMBER,
                "No direct path right now; traffic goes through a relay".to_owned(),
            ))
        } else if let Some(p) = last_ping {
            let (path, col) = if p.direct {
                ("", GREEN)
            } else {
                (" relayed", AMBER)
            };
            Some((
                format!("{:.0} ms{path}", p.latency_ms),
                col,
                "Last ping".to_owned(),
            ))
        } else {
            None
        };

        ui.scope_builder(centered(name_r), |ui| {
            ui.set_clip_rect(name_r.intersect(ui.clip_rect()));
            // Rows hug their text so the two lines center as one block.
            ui.spacing_mut().interact_size.y = 0.0;
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.horizontal(|ui| {
                let name = RichText::new(mask.name(n)).strong().size(14.5);
                ui.label(if dim { name.color(MUTED) } else { name });
                // Blocked access comes first, so it is the last chip to be cut off.
                if let Some((text, color, a)) = &blocked {
                    let (_, _, how) = access_text(a);
                    chip(ui, format!("{}  {text}", icon::BAN), *color).on_hover_text(how);
                }
                if n.is_self {
                    chip(ui, "you", ACCENT);
                }
                if n.exit_node_active {
                    chip(ui, "exit node in use", GREEN);
                } else if n.exit_node_option {
                    chip(ui, "exit node", MUTED);
                }
                for t in &n.tags {
                    chip(ui, format!("{}  {t}", icon::TAG), PURPLE);
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let os = self.os_of(n);
                let title = ui.label(RichText::new(os.title()).size(12.5).color(MUTED));
                let dot = |ui: &mut Ui| {
                    ui.label(RichText::new("·").size(12.5).color(FAINT));
                };
                match os.detail() {
                    Some(d) if self.settings.show_kernel => {
                        dot(ui);
                        ui.label(RichText::new(d).size(12.5).color(FAINT));
                    }
                    Some(_) => {
                        if let Some((k, v)) = os.detail_row() {
                            title.on_hover_text(format!("{k} {v}"));
                        }
                    }
                    None => {}
                }
                // The owner only earns room when it is someone else.
                if n.tags.is_empty() && n.user_login != s.me.user_login {
                    dot(ui);
                    ui.label(
                        RichText::new(mask.who(&n.user_login))
                            .size(12.5)
                            .color(FAINT),
                    );
                }
                if let Some((text, col, tip)) = &status {
                    dot(ui);
                    ui.label(RichText::new(text).size(12.5).color(*col))
                        .on_hover_text(tip);
                }
            });
        });

        if ip_w > 0.0 {
            ui.scope_builder(in_row(ip_r), |ui| {
                let ip = n.ipv4().to_owned();
                if copy_label(ui, &mask.ip(&ip), 13.0) {
                    self.copy(ctx, &ip);
                }
            });
        }

        // Actions appear with the pointer, and only for devices that can use them.
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(act_r)
                .layout(Layout::right_to_left(Align::Center)),
            |ui| {
                let shown = hover || selected;
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                if shown {
                    paint_icon(ui.painter(), r.center(), icon::CHEVRON_RIGHT, 11.0, FAINT);
                }
                if !shown || n.is_self || !n.online {
                    return;
                }
                ui.add_space(4.0);
                if icon_button(ui, icon::SEND, n.taildrop_reason(), n.taildrop_ok()) {
                    self.pick_and_send(ctx, n);
                }
                let running = self.pings.get(&n.stable_id).is_some_and(|p| p.running);
                if icon_button(ui, icon::BOLT, "Ping 5 times", !running) {
                    self.selected = Some(n.stable_id.clone());
                    self.ping(ctx, n);
                }
            },
        );
    }

    fn pick_and_send(&mut self, ctx: &egui::Context, n: &Node) {
        let (ip, name) = (n.ipv4().to_owned(), self.mask.name(n));
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
        // Settings hides the selection, so a drop there has no target.
        let target = self
            .selected
            .as_ref()
            .filter(|_| self.view != View::Settings)
            .and_then(|id| self.snap.as_ref()?.node(id).cloned());
        match target {
            Some(n) if n.taildrop_ok() => {
                let (ip, name) = (n.ipv4().to_owned(), self.mask.name(&n));
                self.act(ctx, &format!("send-{}", n.stable_id), move || {
                    send_all(&dropped, &ip, &name)
                });
            }
            Some(n) => self.toast(
                format!("Can't send to {}: {}", self.mask.name(&n), n.taildrop_reason()),
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
        let mask = self.mask.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(os_icon(&n.os)).size(22.0).color(if n.online { TEXT } else { MUTED }));
                ui.add_space(4.0);
                ui.label(RichText::new(mask.name(&n)).heading());
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
                    chip(ui, format!("{}  {t}", icon::TAG), PURPLE);
                }
                if n.tags.is_empty() && !n.user_login.is_empty() {
                    chip(ui, format!("{}  {}", icon::USER, mask.who(&n.user_login)), MUTED);
                }
            });
            ui.add_space(12.0);

            if n.is_self {
                card(ui, |ui| {
                    section_title(ui, "This device");
                    ui.add_space(4.0);
                    ui.label(RichText::new("Exit node, subnet routes and shields for this device are in Settings.").size(12.5).color(MUTED));
                    ui.add_space(6.0);
                    if ui.button(format!("{}   Open Settings", icon::GEAR)).clicked() {
                        self.view = View::Settings;
                        self.selected = None;
                    }
                });
                ui.add_space(10.0);
            }

            card(ui, |ui| {
                section_title(ui, "Addresses");
                ui.add_space(4.0);
                // Shown masked in privacy mode; a click still copies the real one.
                let mut addrs: Vec<(String, String)> =
                    n.ips.iter().map(|ip| (mask.ip(ip), ip.clone())).collect();
                if !n.dns_name.is_empty() {
                    addrs.push((mask.dns(&n), n.dns_name.clone()));
                }
                for (shown, real) in addrs {
                    if copy_label(ui, &shown, if shown.len() > 26 { 11.5 } else { 13.0 }) {
                        self.copy(&ctx, &real);
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
                                            // A direct path names the public endpoint; hide it whole.
                                            let via = if p.direct && mask.on { "a direct path".to_owned() } else { mask.external(&p.via) };
                                            ui.label(RichText::new(format!("via {via}")).size(12.0).color(MUTED));
                                        });
                                    }
                                    Err(e) => {
                                        ui.label(RichText::new(mask.external(e)).size(12.0).color(RED));
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
                let os = self.os_of(&n);
                let mut rows: Vec<(&str, String, Color32)> = vec![
                    ("Connection", conn, col),
                    ("System", os.title(), TEXT),
                ];
                if let Some((key, v)) = os.detail_row() {
                    rows.push((key, v, TEXT));
                }
                if !os.arch.is_empty() {
                    rows.push(("Architecture", os.arch.clone(), TEXT));
                }
                if !n.is_self && !os.client_version.is_empty() {
                    rows.push(("Tailscale", os.client_version.clone(), TEXT));
                }
                if os.update_available {
                    rows.push(("Update", "a newer Tailscale is available".into(), AMBER));
                }
                rows.extend([
                    ("Hostname", mask.hostname(&n), TEXT),
                    ("Owner", if n.tags.is_empty() { mask.who(&n.user_display) } else { "tagged device".into() }, TEXT),
                ]);
                if !n.cur_addr.is_empty() {
                    rows.push(("Endpoint", mask.secret(&n.cur_addr), TEXT));
                }
                if !n.relay.is_empty() {
                    rows.push(("Home relay", n.relay.clone(), TEXT));
                }
                if !n.is_self {
                    rows.push(("Sent / received", format!("{} / {}", bytes(n.tx), bytes(n.rx)), TEXT));
                }
                if !n.primary_routes.is_empty() {
                    rows.push(("Subnet routes", mask.text(&n.primary_routes.join(", ")), TEXT));
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
                if !n.is_self && !self.api.connected {
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Tailscale shares other devices' OS versions only through its API.").size(11.5).color(FAINT));
                        if ui.link(RichText::new("Connect to see them").size(11.5)).clicked() {
                            self.onboard = Some(Onboarding::at(onboard::OS_STEP));
                        }
                    });
                }
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
            peers.sort_by_key(|p| (!p.taildrop_ok(), self.mask.order(p)));
            for p in peers {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(os_icon(&p.os)).size(16.0));
                        ui.label(RichText::new(self.mask.name(p)).strong());
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

fn access_detail(ui: &mut Ui, what: &str, a: &Access) {
    let (glyph, col, how) = access_text(a);
    ui.horizontal(|ui| {
        ui.label(RichText::new(glyph).color(col));
        ui.label(RichText::new(what).color(TEXT));
    });
    ui.label(RichText::new(how).size(11.5).color(MUTED));
    ui.add_space(4.0);
}
