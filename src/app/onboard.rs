//! First-run walkthrough: what the app is for, what OS versions add and what they
//! cost, setting up the API credential step by step, and a few things worth
//! knowing. It shows once, can be reopened from the sidebar, and every step can
//! be skipped.

use std::cell::Cell;
use std::path::PathBuf;

use eframe::egui::{self, Align, Color32, Layout, Margin, RichText, Stroke, Ui, vec2};

use super::{App, card_frame, display_order, primary_button, step};
use crate::osinfo::{self, OsInfo};
use crate::theme::{icon, *};

const WELCOME: usize = 0;
pub const OS_STEP: usize = 1;
const CONNECT: usize = 2;
const DONE: usize = 3;
const STEPS: usize = 4;
const WIDTH: f32 = 600.0;

/// What a page's buttons asked for, applied once the buttons are drawn.
#[derive(Clone, Copy)]
enum Act {
    Go(usize),
    Close,
    Connect,
}

fn state_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("tailscale-desktop-unofficial").join("onboarded"))
}

/// The step to open on at launch: the first one on a fresh install, or the one
/// `TSDU_ONBOARD` names. Headless captures skip it unless asked for.
pub fn first_step() -> Option<usize> {
    if let Ok(v) = std::env::var("TSDU_ONBOARD") {
        return Some(v.parse().unwrap_or(WELCOME).min(STEPS - 1));
    }
    if std::env::var("TSDU_SHOT").is_ok() {
        return None;
    }
    let done = state_path().is_some_and(|p| p.exists());
    (!done).then_some(WELCOME)
}

pub fn mark_done() -> Result<(), String> {
    if std::env::var("TSDU_SHOT").is_ok() {
        return Ok(());
    }
    let p = state_path().ok_or("no home directory")?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&p, "1\n").map_err(|e| format!("{}: {e}", p.display()))
}

impl App {
    pub(super) fn onboarding(&mut self, ui: &mut Ui) {
        let step = self.onboard.as_ref().map_or(WELCOME, |o| o.step);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let side = ((ui.available_width() - WIDTH) / 2.0).max(24.0);
                let top = (ui.available_height() * 0.14).clamp(24.0, 140.0);
                ui.add_space(top);
                ui.horizontal(|ui| {
                    ui.add_space(side);
                    ui.vertical(|ui| {
                        ui.set_width(WIDTH.min(ui.available_width() - 24.0));
                        progress(ui, step);
                        ui.add_space(28.0);
                        match step {
                            WELCOME => self.page_welcome(ui),
                            OS_STEP => self.page_os(ui),
                            CONNECT => self.page_connect(ui),
                            _ => self.page_done(ui),
                        }
                    });
                });
                ui.add_space(40.0);
            });
    }

    fn act_on(&mut self, act: Option<Act>) {
        match act {
            Some(Act::Go(step)) => self.go(step),
            Some(Act::Close) => self.close_onboarding(),
            Some(Act::Connect) | None => {}
        }
    }

    fn go(&mut self, step: usize) {
        if let Some(o) = &mut self.onboard {
            o.step = step;
            o.error = None;
        }
    }

    fn page_welcome(&mut self, ui: &mut Ui) {
        let (r, _) = ui.allocate_exact_size(vec2(44.0, 44.0), egui::Sense::hover());
        paint_mark(ui.painter(), r);
        ui.add_space(18.0);
        heading(ui, "Your tailnet, on your desktop");
        body(
            ui,
            "Every device you have on Tailscale, whether it is online right now, who can \
             reach whom, and the tools to ping a device or send it files. It all comes \
             straight from tailscaled on this machine.",
        );
        ui.add_space(26.0);

        // The user's own devices, drawn the way the app draws them everywhere.
        if let Some(s) = &self.snap {
            let mut nodes: Vec<_> = s.all().collect();
            nodes.sort_by_key(|n| display_order(n));
            nodes.truncate(8);
            let slot = 74.0;
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 92.0), egui::Sense::hover());
            let x0 = rect.center().x - slot * nodes.len() as f32 / 2.0 + slot / 2.0;
            for (i, n) in nodes.iter().enumerate() {
                let c = egui::pos2(x0 + slot * i as f32, rect.top() + 30.0);
                paint_badge(ui.painter(), c, 22.0, os_icon(&n.os), Badge::of(n), 1.0);
                let name: String = if n.name.chars().count() > 10 {
                    n.name.chars().take(9).chain(['…']).collect()
                } else {
                    n.name.clone()
                };
                ui.painter().text(
                    egui::pos2(c.x, c.y + 34.0),
                    egui::Align2::CENTER_TOP,
                    name,
                    egui::FontId::proportional(12.0),
                    if n.online || n.is_self { TEXT } else { MUTED },
                );
            }
            ui.add_space(8.0);
            let online = s.all().filter(|n| n.online || n.is_self).count();
            note(
                ui,
                &format!(
                    "A green ring means the device is online. {online} of {} are right now.",
                    s.all().count()
                ),
            );
        }

        ui.add_space(18.0);
        note(
            ui,
            "Tailscale Desktop is an unofficial app, made independently and not by Tailscale. \
             It only talks to the Tailscale already running on this machine.",
        );
        ui.add_space(24.0);
        let act = Cell::new(None);
        buttons(
            ui,
            |ui| {
                if primary_button(ui, "Get started").clicked() {
                    act.set(Some(Act::Go(OS_STEP)));
                }
            },
            |ui| {
                if ui.button("Skip the tour").clicked() {
                    act.set(Some(Act::Close));
                }
            },
        );
        self.act_on(act.get());
    }

    fn page_os(&mut self, ui: &mut Ui) {
        heading(ui, "Know what every device runs");
        let me = self
            .snap
            .as_ref()
            .map(|s| s.me.name.clone())
            .unwrap_or_default();
        body(
            ui,
            &format!(
                "{me} already knows its own distro and kernel. For your other devices, \
                 Tailscale only shares that through its web API."
            ),
        );
        ui.add_space(20.0);

        // Before and after, with real devices: this machine as it already shows,
        // and a peer as it shows now.
        if let Some(s) = &self.snap {
            let peer = s
                .peers
                .iter()
                .find(|p| p.online && p.tags.is_empty())
                .or(s.peers.first());
            panel(ui, |ui| {
                preview_row(ui, &s.me, &self.os_of(&s.me), None);
                if let Some(p) = peer {
                    divider(ui);
                    let hint = (!self.api.connected).then_some("full version after setup");
                    preview_row(ui, p, &self.os_of(p), hint);
                }
            });
        }

        ui.add_space(22.0);
        ui.label(RichText::new("What it takes").strong().size(15.0));
        ui.add_space(6.0);
        for line in [
            "One credential, made once in the Tailscale admin console. About a minute.",
            "It can read device details and nothing else. It can't change your tailnet.",
            "It is kept in your keyring, and Disconnect in the sidebar removes it.",
        ] {
            ui.horizontal_top(|ui| {
                ui.label(RichText::new(icon::CHECK).color(GREEN));
                ui.add(egui::Label::new(RichText::new(line).color(MUTED)).wrap());
            });
            ui.add_space(2.0);
        }

        ui.add_space(26.0);
        let connected = self.api.connected;
        let act = Cell::new(None);
        buttons(
            ui,
            |ui| {
                let label = if connected { "Continue" } else { "Set it up" };
                if primary_button(ui, label).clicked() {
                    act.set(Some(Act::Go(if connected { DONE } else { CONNECT })));
                }
            },
            |ui| {
                if ui.button("Back").clicked() {
                    act.set(Some(Act::Go(WELCOME)));
                }
                if !connected && ui.button("Skip for now").clicked() {
                    act.set(Some(Act::Go(DONE)));
                }
            },
        );
        self.act_on(act.get());
    }

    fn page_connect(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.onboard.as_ref().is_some_and(|o| o.connected) {
            return self.page_connected(ui);
        }
        heading(ui, "Make the credential");
        body(
            ui,
            "You need to be an owner or admin of the tailnet. Keep this window open next \
             to the browser.",
        );
        ui.add_space(20.0);

        let mut go = false;
        panel(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 14.0;
            step(
                ui,
                "1",
                "Open the Trust credentials page in the admin console.",
                |ui| {
                    ui.add_space(4.0);
                    if ui
                        .button(format!("{}   Open admin console", icon::GLOBE))
                        .clicked()
                    {
                        ui.ctx()
                            .open_url(egui::OpenUrl::new_tab(osinfo::CONSOLE_URL));
                    }
                },
            );
            step(ui, "2", "Press Credential, then choose OAuth.", |_| {});
            step(
                ui,
                "3",
                "Set Devices Core and Device posture to Read. Leave everything else off.",
                |_| {},
            );
            step(
                ui,
                "4",
                "Press Generate credential and copy the client secret. Tailscale shows it only once.",
                |_| {},
            );
            let o = self.onboard.as_mut().unwrap();
            step(ui, "5", "Paste it here.", |ui| {
                ui.add_space(4.0);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut o.secret)
                        .password(true)
                        .hint_text("tskey-client-...")
                        .margin(Margin::symmetric(10, 7))
                        .desired_width(f32::INFINITY),
                );
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    go = true;
                }
            });
        });

        let o = self.onboard.as_ref().unwrap();
        let valid = osinfo::client_id(&o.secret).is_some();
        let busy = o.busy;
        ui.add_space(10.0);
        if !o.secret.trim().is_empty() && !valid {
            note_color(ui, "A client secret starts with tskey-client-.", AMBER);
        }
        if let Some(e) = &o.error {
            note_color(ui, e, RED);
        }

        ui.add_space(20.0);
        let act = Cell::new(None);
        buttons(
            ui,
            |ui| {
                let label = if busy { "Checking" } else { "Connect" };
                if ui
                    .add_enabled_ui(valid && !busy, |ui| primary_button(ui, label))
                    .inner
                    .clicked()
                {
                    act.set(Some(Act::Connect));
                }
                if busy {
                    ui.spinner();
                }
            },
            |ui| {
                if ui.button("Back").clicked() {
                    act.set(Some(Act::Go(OS_STEP)));
                }
                if ui.button("Skip for now").clicked() {
                    act.set(Some(Act::Go(DONE)));
                }
            },
        );
        let act = act.get().or(go.then_some(Act::Connect));
        match act {
            Some(Act::Connect) if valid && !busy => self.start_connect(&ctx),
            Some(Act::Connect) => {}
            Some(Act::Go(to)) => {
                // Leaving the page drops a check that is still running.
                self.connect_attempt += 1;
                if let Some(o) = &mut self.onboard {
                    o.busy = false;
                }
                self.go(to);
            }
            other => self.act_on(other),
        }
    }

    /// The payoff, right after connecting: what the API said about each device.
    fn page_connected(&mut self, ui: &mut Ui) {
        heading(ui, "Connected");
        body(
            ui,
            &format!(
                "Tailscale answered for {} devices. This is what the app shows from now on.",
                self.os_api.len()
            ),
        );
        // The list needs tailscaled; the way on does not.
        if let Some(s) = &self.snap {
            ui.add_space(20.0);
            let mut nodes: Vec<_> = s.all().collect();
            nodes.sort_by_key(|n| display_order(n));
            panel(ui, |ui| {
                let last = nodes.len().saturating_sub(1);
                for (i, n) in nodes.iter().enumerate() {
                    preview_row(ui, n, &self.os_of(n), None);
                    if i != last {
                        divider(ui);
                    }
                }
            });
        }
        ui.add_space(24.0);
        let mut next = false;
        buttons(
            ui,
            |ui| next = primary_button(ui, "Continue").clicked(),
            |_| {},
        );
        if next {
            self.go(DONE);
        }
    }

    fn page_done(&mut self, ui: &mut Ui) {
        heading(ui, "You're set");
        body(ui, "A few things that are easy to miss:");
        ui.add_space(18.0);
        panel(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 12.0;
            for (glyph, text) in [
                (icon::COPY, "Click any address or MagicDNS name to copy it."),
                (
                    icon::SEND,
                    "Drop files anywhere on the window to send them to a device.",
                ),
                (
                    icon::GRAPH,
                    "The access graph shows who can reach whom. Hover a device to see its links.",
                ),
                (
                    icon::BOLT,
                    "Ping shows the latency and whether the path is direct or relayed.",
                ),
            ] {
                ui.horizontal_top(|ui| {
                    ui.add_sized(
                        vec2(22.0, 18.0),
                        egui::Label::new(RichText::new(glyph).color(ACCENT)),
                    );
                    ui.add(egui::Label::new(RichText::new(text).color(TEXT)).wrap());
                });
            }
        });
        if !self.api.connected {
            ui.add_space(12.0);
            note(
                ui,
                "OS versions stay off for now. Set them up any time from the sidebar.",
            );
        }
        ui.add_space(26.0);
        let act = Cell::new(None);
        buttons(
            ui,
            |ui| {
                if primary_button(ui, "Open Tailscale Desktop").clicked() {
                    act.set(Some(Act::Close));
                }
            },
            |ui| {
                if ui.button("Back").clicked() {
                    act.set(Some(Act::Go(OS_STEP)));
                }
            },
        );
        self.act_on(act.get());
    }
}

/// Where you are in the walkthrough: one bar per page, filled up to this one.
fn progress(ui: &mut Ui, step: usize) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.0), egui::Sense::hover());
    let gap = 6.0;
    let w = (rect.width() - gap * (STEPS - 1) as f32) / STEPS as f32;
    for i in 0..STEPS {
        let r = egui::Rect::from_min_size(
            egui::pos2(rect.left() + i as f32 * (w + gap), rect.top()),
            vec2(w, 4.0),
        );
        let col = if i <= step { ACCENT } else { BORDER };
        ui.painter().rect_filled(r, 2.0, col);
    }
}

fn heading(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(28.0)
            .family(egui::FontFamily::Name("strong".into()))
            .color(TEXT),
    );
    ui.add_space(6.0);
}

fn body(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(15.0).color(MUTED)).wrap());
}

fn note(ui: &mut Ui, text: &str) {
    note_color(ui, text, FAINT);
}

fn note_color(ui: &mut Ui, text: &str, color: Color32) {
    ui.add(egui::Label::new(RichText::new(text).size(13.0).color(color)).wrap());
}

fn panel(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    card_frame(16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    });
}

fn divider(ui: &mut Ui) {
    ui.add_space(8.0);
    let r = ui.available_rect_before_wrap();
    ui.painter().hline(
        r.left() + 46.0..=r.right(),
        r.top(),
        Stroke::new(1.0, BORDER),
    );
    ui.add_space(9.0);
}

/// A device as the list shows it: ring, name, and what it runs.
fn preview_row(ui: &mut Ui, n: &crate::ts::Node, os: &OsInfo, hint: Option<&str>) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), egui::Sense::hover());
        paint_badge(
            ui.painter(),
            r.center(),
            15.0,
            os_icon(&n.os),
            Badge::of(n),
            1.0,
        );
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(&n.name).strong());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(RichText::new(os.title()).size(12.5).color(MUTED));
                if let Some(d) = os.detail() {
                    ui.label(RichText::new(d).size(12.5).color(FAINT));
                }
                if let Some(h) = hint {
                    ui.label(RichText::new(h).size(12.5).italics().color(FAINT));
                }
            });
        });
    });
}

/// Primary action on the right, the quieter ones on the left.
fn buttons(ui: &mut Ui, right: impl FnOnce(&mut Ui), left: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        left(ui);
        ui.with_layout(Layout::right_to_left(Align::Center), right);
    });
}
