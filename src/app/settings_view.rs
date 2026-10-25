//! Settings: how this device routes traffic, privacy mode, what the device list
//! shows, OS versions, help and version info. Everything that used to sit in
//! the sidebar and the header beside the navigation.

use eframe::egui::{self, Align, Layout, RichText, Stroke, Ui, UiBuilder, vec2};
use serde_json::json;

use super::{App, Onboarding, card_frame, onboard, primary_button, toggle};
use crate::theme::*;
use crate::ts::{self, Node, Snapshot};

const WIDTH: f32 = 680.0;

impl App {
    pub(super) fn settings_view(&mut self, ui: &mut Ui) {
        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
        // Test hook: captures can't scroll, so `TSDU_SCROLL` holds it there.
        if let Some(y) = std::env::var("TSDU_SCROLL")
            .ok()
            .and_then(|v| v.parse().ok())
        {
            area = area.vertical_scroll_offset(y);
        }
        area.show(ui, |ui| {
            ui.set_max_width(WIDTH.min(ui.available_width()));
            ui.label(RichText::new("Settings").heading());
            ui.add_space(14.0);
            // Prefs can change while Tailscale is off, ready for when it is back.
            match self.snap.clone() {
                Some(s) => self.routing(ui, &s),
                None => group(ui, "This device", |ui| {
                    ui.label(
                        RichText::new("Routing settings show once tailscaled is running.")
                            .color(MUTED),
                    );
                }),
            }
            self.privacy(ui);
            self.list_options(ui);
            self.os_versions(ui);
            self.help(ui);
            self.about(ui);
            ui.add_space(24.0);
        });
    }

    fn routing(&mut self, ui: &mut Ui, s: &Snapshot) {
        let ctx = ui.ctx().clone();
        group(ui, "This device", |ui| {
            let current = s
                .peers
                .iter()
                .find(|p| p.stable_id == s.prefs.exit_node_id || p.exit_node_active);
            row(
                ui,
                "Exit node",
                "Send all internet traffic from this device through another one.",
                |ui| {
                    if let Some(c) = self.exit_picker(ui, s, current) {
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
                },
            );
            if current.is_some() {
                divider(ui);
                let on = s.prefs.exit_node_allow_lan_access;
                row(
                    ui,
                    "Allow LAN access",
                    "Keep reaching your local network while using the exit node.",
                    |ui| {
                        if toggle(ui, on, false).clicked() {
                            self.act(&ctx, "lan", move || {
                                ts::patch_prefs(json!({"ExitNodeAllowLANAccess": !on, "ExitNodeAllowLANAccessSet": true}))
                                    .map(|_| String::new())
                            });
                        }
                    },
                );
            }
            divider(ui);
            let on = s.prefs.route_all;
            row(
                ui,
                "Use subnet routes",
                "Reach the networks that other devices share on the tailnet.",
                |ui| {
                    if toggle(ui, on, false).clicked() {
                        self.act(&ctx, "routes", move || {
                            ts::patch_prefs(json!({"RouteAll": !on, "RouteAllSet": true}))
                                .map(|_| String::new())
                        });
                    }
                },
            );
            divider(ui);
            let on = s.prefs.advertises_exit();
            row(
                ui,
                "Offer as exit node",
                "Let other devices send their internet traffic through this one.",
                |ui| {
                    if toggle(ui, on, false).clicked() {
                        let prefs = s.prefs.clone();
                        self.act(&ctx, "adv", move || {
                            ts::set_advertise_exit(&prefs, !on).map(|_| String::new())
                        });
                    }
                },
            );
            divider(ui);
            let on = s.prefs.shields_up;
            row(
                ui,
                "Shields up",
                "Block every incoming connection to this device.",
                |ui| {
                    if toggle(ui, on, false).clicked() {
                        self.act(&ctx, "shields", move || {
                            ts::patch_prefs(json!({"ShieldsUp": !on, "ShieldsUpSet": true}))
                                .map(|_| String::new())
                        });
                    }
                },
            );
        });
    }

    /// The exit node picker; returns a choice when one was made.
    fn exit_picker(
        &self,
        ui: &mut Ui,
        s: &Snapshot,
        current: Option<&Node>,
    ) -> Option<Option<String>> {
        let exits: Vec<&Node> = s.peers.iter().filter(|p| p.exit_node_option).collect();
        let mut choice = None;
        egui::ComboBox::from_id_salt("exit")
            .width(200.0)
            .selected_text(
                current
                    .map(|c| self.mask.name(c))
                    .unwrap_or_else(|| "None".into()),
            )
            .show_ui(ui, |ui| {
                if ui.selectable_label(current.is_none(), "None").clicked() {
                    choice = Some(None);
                }
                for e in &exits {
                    let name = self.mask.name(e);
                    let label = if e.online {
                        name
                    } else {
                        format!("{name} (offline)")
                    };
                    let sel = current.is_some_and(|c| c.stable_id == e.stable_id);
                    if ui.selectable_label(sel, label).clicked() {
                        choice = Some(Some(e.stable_id.clone()));
                    }
                }
                if exits.is_empty() {
                    ui.label(RichText::new("No device offers an exit node").color(MUTED));
                }
            });
        choice
    }

    fn privacy(&mut self, ui: &mut Ui) {
        let (on, names) = (self.settings.privacy, self.settings.privacy_names);
        let mut flip_privacy = false;
        let mut flip_names = false;
        group(ui, "Privacy", |ui| {
            row(
                ui,
                "Privacy mode",
                "Hide IP addresses, accounts, owners and endpoints, for streams and \
                 screenshots. Also the eye in the header, or Ctrl+Shift+P.",
                |ui| flip_privacy = toggle(ui, on, false).clicked(),
            );
            divider(ui);
            row(
                ui,
                "Hide device names too",
                "While privacy mode is on, devices show as device-1, device-2 and so on.",
                |ui| flip_names = toggle(ui, names, false).clicked(),
            );
        });
        if flip_privacy {
            self.toggle_privacy();
        }
        if flip_names {
            self.set(|s| s.privacy_names = !s.privacy_names);
        }
    }

    fn list_options(&mut self, ui: &mut Ui) {
        let (ips, kernel) = (self.settings.show_ips, self.settings.show_kernel);
        let (mut flip_ips, mut flip_kernel) = (false, false);
        group(ui, "Device list", |ui| {
            row(
                ui,
                "IP addresses",
                "Each device's Tailscale IP, which copies on click.",
                |ui| flip_ips = toggle(ui, ips, false).clicked(),
            );
            divider(ui);
            row(
                ui,
                "Kernel and build",
                "Next to the OS name. Hover the OS name to see it either way.",
                |ui| flip_kernel = toggle(ui, kernel, false).clicked(),
            );
        });
        if flip_ips {
            self.set(|s| s.show_ips = !s.show_ips);
        }
        if flip_kernel {
            self.set(|s| s.show_kernel = !s.show_kernel);
        }
    }

    fn os_versions(&mut self, ui: &mut Ui) {
        let mut remove = false;
        let mut set_up = false;
        group(ui, "OS versions", |ui| {
            if self.api.connected {
                let (text, col) = match &self.api.error {
                    None => ("On for every device", GREEN),
                    Some(_) => ("Could not refresh", AMBER),
                };
                row(
                    ui,
                    RichText::new(text).color(col),
                    "Read from the Tailscale API with a read-only credential kept in your \
                     keyring. Removing it doesn't touch your connection to the tailnet.",
                    |ui| {
                        remove = ui
                            .button("Remove credential")
                            .on_hover_text("Delete the API credential from the keyring")
                            .clicked();
                    },
                );
                if let Some(e) = &self.api.error {
                    ui.label(RichText::new(self.mask.external(e)).size(11.5).color(FAINT));
                }
            } else {
                row(
                    ui,
                    "Only this device",
                    "Other devices share their distro, kernel and OS version only through \
                     the Tailscale API. Setup takes about a minute.",
                    |ui| set_up = primary_button(ui, "Set up").clicked(),
                );
            }
        });
        if remove {
            self.disconnect();
        }
        if set_up {
            self.onboard = Some(Onboarding::at(onboard::OS_STEP));
        }
    }

    fn help(&mut self, ui: &mut Ui) {
        let mut tour = false;
        group(ui, "Help", |ui| {
            row(
                ui,
                "Welcome tour",
                "What the app does and the things that are easy to miss.",
                |ui| tour = ui.button("Open").clicked(),
            );
            divider(ui);
            row(
                ui,
                "Search",
                "tag:server, os:android, user:alice and is:offline narrow the device list.",
                |_| {},
            );
        });
        if tour {
            self.onboard = Some(Onboarding::at(0));
        }
    }

    fn about(&mut self, ui: &mut Ui) {
        let mut rows = vec![(
            "App",
            format!("{} {}", crate::APP_NAME, env!("CARGO_PKG_VERSION")),
        )];
        if let Some(s) = &self.snap {
            let op = if s.prefs.operator_user.is_empty() {
                "root"
            } else {
                &s.prefs.operator_user
            };
            rows.extend([
                ("Tailnet", self.mask.who(&s.tailnet)),
                ("tailscaled", s.version.clone()),
                ("Operator", self.mask.who(op)),
            ]);
        }
        group(ui, "About", |ui| {
            egui::Grid::new("about")
                .num_columns(2)
                .spacing(vec2(16.0, 6.0))
                .show(ui, |ui| {
                    for (k, v) in rows {
                        ui.label(RichText::new(k).size(12.5).color(MUTED));
                        ui.label(RichText::new(v).size(12.5));
                        ui.end_row();
                    }
                });
            ui.add_space(6.0);
            ui.label(
                RichText::new("Made independently, not by Tailscale.")
                    .size(11.5)
                    .color(FAINT),
            );
        });
    }
}

/// A titled card of settings.
fn group(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    ui.label(RichText::new(title).size(12.5).color(MUTED).strong());
    ui.add_space(6.0);
    card_frame(16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    });
    ui.add_space(20.0);
}

/// A setting: its name with a line on what it does, and its control on the right.
fn row(
    ui: &mut Ui,
    label: impl Into<egui::WidgetText>,
    hint: &str,
    control: impl FnOnce(&mut Ui),
) {
    let control_w = 220.0;
    let left = ui.cursor().left();
    let full = ui.available_width();
    let text_w = (full - control_w).max(160.0);
    let text = ui
        .allocate_ui_with_layout(vec2(text_w, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.set_width(text_w);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(label);
            ui.add(egui::Label::new(RichText::new(hint).size(12.5).color(MUTED)).wrap());
        })
        .response
        .rect;
    // The control centers on the text beside it, however many lines that took.
    let slot = egui::Rect::from_min_max(
        egui::pos2(left + text_w, text.top()),
        egui::pos2(left + full, text.bottom()),
    );
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(slot)
            .layout(Layout::right_to_left(Align::Center)),
        control,
    );
}

fn divider(ui: &mut Ui) {
    ui.add_space(6.0);
    let r = ui.available_rect_before_wrap();
    ui.painter()
        .hline(r.left()..=r.right(), r.top(), Stroke::new(1.0, BORDER));
    ui.add_space(7.0);
}
