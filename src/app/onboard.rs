//! First-run walkthrough: what the app is for, what OS versions add and what they
//! cost, setting up the API credential step by step, and a few things worth
//! knowing. It shows once, can be reopened from Settings or the status menu, and
//! every step can be skipped.

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
    Some(crate::settings::config_dir()?.join("onboarded"))
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
                // Center the page in the window, from its height last frame.
                let id = ui.id().with("onboard-height");
                let last: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(0.0);
                let top = ((ui.available_height() - last) / 2.0).clamp(24.0, 200.0);
                ui.add_space(top);
                let page = ui.horizontal(|ui| {
                    ui.add_space(side);
                    ui.vertical(|ui| {
                        ui.set_width(WIDTH.min(ui.available_width() - 24.0));
                        progress(ui, step);
                        ui.add_space(10.0);
                        match step {
                            WELCOME => self.page_welcome(ui),
                            OS_STEP => self.page_os(ui),
                            CONNECT => self.page_connect(ui),
                            _ => self.page_done(ui),
                        }
                    });
                });
                let h = page.response.rect.height();
                ui.ctx().data_mut(|d| d.insert_temp(id, h));
                // A page of a new height is placed right on the next frame.
                if (h - last).abs() > 1.0 {
                    ui.ctx().request_repaint();
                }
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
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(30.0, 30.0), egui::Sense::hover());
            paint_mark(ui.painter(), r);
            ui.add_space(8.0);
            ui.label(
                RichText::new(crate::APP_NAME)
                    .size(15.0)
                    .family(egui::FontFamily::Name("strong".into()))
                    .color(TEXT),
            );
        });
        ui.add_space(22.0);
        heading(ui, "Your tailnet, on your desktop");
        body(
            ui,
            "See every device on your tailnet, which ones are online and who can reach \
             whom. Ping a device or send it files in one click.",
        );
        ui.add_space(24.0);

        // The user's own devices, drawn the way the app draws them everywhere.
        if let Some(s) = &self.snap {
            let mut nodes: Vec<_> = s.all().collect();
            nodes.sort_by_key(|n| display_order(n));
            nodes.truncate(8);
            let slot = (ui.available_width() / nodes.len().max(1) as f32).min(84.0);
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 104.0), egui::Sense::hover());
            let x0 = rect.center().x - slot * nodes.len() as f32 / 2.0 + slot / 2.0;
            for (i, n) in nodes.iter().enumerate() {
                let c = egui::pos2(x0 + slot * i as f32, rect.top() + 26.0);
                paint_badge(ui.painter(), c, 19.0, os_icon(&n.os), labelled(n), 1.0);
                // Two lines of name before it shortens, and the full name on hover.
                let color = if n.online || n.is_self { TEXT } else { MUTED };
                let mut job = egui::text::LayoutJob::single_section(
                    self.mask.name(n),
                    egui::TextFormat::simple(egui::FontId::proportional(11.5), color),
                );
                job.wrap = egui::text::TextWrapping {
                    max_width: slot - 6.0,
                    max_rows: 2,
                    break_anywhere: false,
                    overflow_character: Some('…'),
                };
                job.halign = Align::Center;
                let g = ui.painter().layout_job(job);
                let top = c.y + 28.0;
                let name_h = g.size().y;
                ui.painter().galley(egui::pos2(c.x, top), g, color);
                if n.is_self {
                    ui.painter().text(
                        egui::pos2(c.x, top + name_h + 1.0),
                        egui::Align2::CENTER_TOP,
                        "this device",
                        egui::FontId::proportional(10.5),
                        ACCENT,
                    );
                }
                let hit = egui::Rect::from_center_size(
                    egui::pos2(c.x, c.y + 30.0),
                    vec2(slot - 4.0, 104.0),
                );
                ui.interact(hit, ui.id().with(("welcome-node", i)), egui::Sense::hover())
                    .on_hover_text(self.mask.name(n));
            }
            let online = s.all().filter(|n| n.online || n.is_self).count();
            let tagged = nodes.iter().any(|n| !n.tags.is_empty());
            note(
                ui,
                &format!(
                    "{online} of {} online.{}",
                    s.all().count(),
                    if tagged {
                        " Squares are tagged devices."
                    } else {
                        ""
                    }
                ),
            );
        }

        ui.add_space(40.0);
        ui.add(
            egui::Label::new(
                RichText::new(
                    "Made independently, not by Tailscale. Everything comes from the \
                     Tailscale already running on this machine.",
                )
                .size(11.0)
                .color(FAINT.gamma_multiply(0.85)),
            )
            .wrap(),
        );
        ui.add_space(22.0);
        let act = Cell::new(None);
        buttons(
            ui,
            |ui| {
                if primary_button(ui, "Get started").clicked() {
                    act.set(Some(Act::Go(OS_STEP)));
                }
            },
            |ui| {
                if quiet_button(ui, "Skip the tour").clicked() {
                    act.set(Some(Act::Close));
                }
            },
        );
        self.act_on(act.get());
    }

    fn page_os(&mut self, ui: &mut Ui) {
        heading(ui, "See what every device runs");
        body(
            ui,
            "Other devices need one extra permission from Tailscale before they share \
             what they run.",
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
                let kernel = self.settings.show_kernel;
                preview_row(ui, &s.me, &self.mask.name(&s.me), &self.os_of(&s.me), kernel, None);
                if let Some(p) = peer {
                    divider(ui);
                    let hint = (!self.api.connected).then_some("available after setup");
                    preview_row(ui, p, &self.mask.name(p), &self.os_of(p), kernel, hint);
                }
            });
        }

        ui.add_space(16.0);
        ui.label(RichText::new("What it takes").strong().size(15.0));
        ui.add_space(6.0);
        for (glyph, line) in [
            (
                icon::CLOCK,
                "An access token, called a trust credential in the admin console. \
                 Takes about a minute.",
            ),
            (
                icon::EYE,
                "It can read your devices and settings, and it can't change anything.",
            ),
            (icon::KEY, "It is stored in your keyring."),
            (
                icon::PLUG,
                "Remove it any time from Settings.",
            ),
        ] {
            ui.horizontal_top(|ui| {
                ui.add_sized(
                    vec2(20.0, 18.0),
                    egui::Label::new(RichText::new(glyph).color(MUTED)),
                );
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
                if !connected && quiet_button(ui, "Skip for now").clicked() {
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
            "You need to be an owner or admin of the tailnet. Here is every click, \
             played on a copy of the console. Open it next to this window and follow along.",
        );
        ui.add_space(20.0);

        let mut go = false;
        let o = self.onboard.as_mut().unwrap();
        panel(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("In the admin console").strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if primary_button(ui, &format!("{}   Open admin console", icon::GLOBE))
                        .clicked()
                    {
                        ui.ctx()
                            .open_url(egui::OpenUrl::new_tab(osinfo::CONSOLE_URL));
                    }
                });
            });
            ui.add_space(12.0);
            super::replay::show(ui, &mut o.replay);
            ui.add_space(6.0);
            divider(ui);
            step(ui, "6", "Paste the client secret here.", |ui| {
                ui.add_space(6.0);
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
                if quiet_button(ui, "Skip for now").clicked() {
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
                    let name = self.mask.name(n);
                    preview_row(ui, n, &name, &self.os_of(n), self.settings.show_kernel, None);
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
                "OS versions stay off for now. Set them up any time from Settings.",
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

/// A plain text button for the way out of a page, so the primary action leads.
fn quiet_button(ui: &mut Ui, text: &str) -> egui::Response {
    ui.scope(|ui| {
        // Muted at rest, the normal text color on hover, so it still reads as a button.
        ui.visuals_mut().widgets.inactive.fg_stroke.color = MUTED;
        ui.add(egui::Button::new(text).frame(false))
    })
    .inner
}

/// Where you are in the walkthrough: one bar per page, filled up to this one,
/// with the count under it.
fn progress(ui: &mut Ui, step: usize) {
    let (all, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), egui::Sense::hover());
    let rect = egui::Rect::from_min_size(all.min, vec2(all.width(), 4.0));
    ui.painter().text(
        egui::pos2(rect.right(), rect.bottom() + 5.0),
        egui::Align2::RIGHT_TOP,
        format!("{} of {STEPS}", step + 1),
        egui::FontId::proportional(11.5),
        FAINT,
    );
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

/// A device's badge where its name is labelled: the label marks this device,
/// so the badge drops its fill.
fn labelled(n: &crate::ts::Node) -> Badge {
    Badge {
        me: false,
        ..Badge::of(n)
    }
}

/// A device as the list shows it: ring, name, and what it runs.
fn preview_row(
    ui: &mut Ui,
    n: &crate::ts::Node,
    name: &str,
    os: &OsInfo,
    kernel: bool,
    hint: Option<&str>,
) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), egui::Sense::hover());
        paint_badge(
            ui.painter(),
            r.center(),
            15.0,
            os_icon(&n.os),
            labelled(n),
            1.0,
        );
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).strong());
                if n.is_self {
                    ui.label(RichText::new("this device").size(11.5).color(ACCENT));
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                ui.label(RichText::new(os.title()).size(12.5).color(MUTED));
                if let Some(d) = os.detail().filter(|_| kernel) {
                    ui.label(RichText::new(d).size(12.5).color(FAINT));
                }
                // What setup adds, shown as locked until then.
                if let Some(h) = hint {
                    ui.label(
                        RichText::new(format!("{}   {h}", icon::LOCK))
                            .size(12.5)
                            .color(MUTED),
                    );
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
