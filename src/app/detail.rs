//! The side panel for one device. Beside the device list it is about the device
//! itself; beside the access graph it is about who the device can reach.

use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Ui, vec2};

use super::{
    App, Onboarding, View, access_text, ago, bytes, connection, copy_label, local_time, onboard,
};
use crate::access::Access;
use crate::theme::{icon, *};
use crate::ts::{self, Node};
use crate::widgets::{self, Btn};

/// Hostnames like "0,1x Engineer" next to the name "01x-engineer" say nothing new.
fn same_name(a: &str, b: &str) -> bool {
    let norm = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    norm(a) == norm(b)
}

impl App {
    /// Badge, name and one line of status, with the close button on the right.
    fn panel_header(&mut self, ui: &mut Ui, n: &Node, sub: &str) {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
            let os_icon = self.icon_of(n);
            paint_badge(
                ui.painter(),
                r.center(),
                19.0,
                os_icon.glyph,
                Badge::of(n),
                1.0,
            );
            ui.interact(r, ui.id().with("panel-badge"), Sense::hover())
                .on_hover_text(&os_icon.label);
            ui.add_space(sp::XS);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add_space(1.0);
                ui.add(egui::Label::new(RichText::new(self.mask.name(n)).heading()).truncate());
                ui.label(RichText::new(sub).size(ty::SMALL).color(MUTED));
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                if ui
                    .add(Btn::icon(icon::CLOSE).small())
                    .on_hover_text("Close")
                    .clicked()
                {
                    self.selected = None;
                }
            });
        });
    }

    /// The system and whether the device is around, for under its name.
    fn status_line(&self, n: &Node) -> String {
        let os = self.os_of(n).title();
        let state = if n.is_self {
            "this device".to_owned()
        } else if n.online {
            "online".to_owned()
        } else {
            format!("seen {}", ago(n.last_seen))
        };
        format!("{os}  ·  {state}")
    }

    pub(super) fn detail(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let s = self.snap.clone().unwrap();
        let Some(n) = self.selected.as_ref().and_then(|id| s.node(id)).cloned() else {
            return;
        };
        let mask = self.mask.clone();
        let sub = self.status_line(&n);
        self.panel_header(ui, &n, &sub);
        ui.add_space(sp::M);

        // The two things you do with a device, right under its name.
        ui.horizontal(|ui| {
            if n.is_self {
                if ui
                    .add(Btn::new("Open Settings").glyph(icon::GEAR))
                    .on_hover_text("Exit node, subnet routes and shields for this device")
                    .clicked()
                {
                    self.view = View::Settings;
                    self.selected = None;
                }
                return;
            }
            let running = self.pings.get(&n.stable_id).is_some_and(|p| p.running);
            let ping = ui
                .add_enabled(n.online && !running, Btn::new("Ping").glyph(icon::BOLT))
                .on_hover_text("Five pings: latency, and whether the path is direct")
                .on_disabled_hover_text(if n.online {
                    "Pinging"
                } else {
                    "Offline, nothing to ping"
                });
            if ping.clicked() {
                self.ping(&ctx, &n);
            }
            let busy = self.busy.contains(&format!("send-{}", n.stable_id));
            let send = ui
                .add_enabled(
                    n.taildrop_ok() && !busy,
                    Btn::new("Send files").glyph(icon::SEND),
                )
                .on_hover_text("Or drop files anywhere on this window")
                .on_disabled_hover_text(n.taildrop_reason());
            if send.clicked() {
                self.pick_and_send(&ctx, &n);
            }
            if running || busy {
                ui.add(egui::Spinner::new());
            }
        });

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(log) = self
                    .pings
                    .get(&n.stable_id)
                    .filter(|l| !l.results.is_empty())
                {
                    ui.add_space(sp::M);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for r in &log.results {
                        match r {
                            Ok(p) => {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("{:>6.1} ms", p.latency_ms))
                                            .monospace()
                                            .color(if p.direct { GREEN } else { AMBER }),
                                    );
                                    // A direct path names the public endpoint; hide it whole.
                                    let via = if p.direct && mask.on {
                                        "a direct path".to_owned()
                                    } else {
                                        mask.external(&p.via)
                                    };
                                    ui.label(
                                        RichText::new(format!("via {via}"))
                                            .size(ty::SMALL)
                                            .color(MUTED),
                                    );
                                });
                            }
                            Err(e) => {
                                ui.label(
                                    RichText::new(mask.external(e)).size(ty::SMALL).color(RED),
                                );
                            }
                        }
                    }
                    let ok: Vec<f64> = log
                        .results
                        .iter()
                        .filter_map(|r| r.as_ref().ok().map(|p| p.latency_ms))
                        .collect();
                    if !ok.is_empty() && !log.running {
                        let min = ok.iter().cloned().fold(f64::MAX, f64::min);
                        let avg = ok.iter().sum::<f64>() / ok.len() as f64;
                        ui.add_space(sp::XS);
                        ui.label(
                            RichText::new(format!(
                                "{}/{} replies, min {min:.1} ms, avg {avg:.1} ms",
                                ok.len(),
                                log.results.len()
                            ))
                            .size(ty::SMALL),
                        );
                    }
                    ui.spacing_mut().item_spacing.y = sp::S;
                }

                widgets::divider(ui, sp::M);
                widgets::section(ui, "Addresses");
                // Shown masked in privacy mode; a click still copies the real one.
                let mut addrs: Vec<(String, String)> =
                    n.ips.iter().map(|ip| (mask.ip(ip), ip.clone())).collect();
                if !n.dns_name.is_empty() {
                    addrs.push((mask.dns(&n), n.dns_name.clone()));
                }
                ui.spacing_mut().item_spacing.y = 0.0;
                for (shown, real) in addrs {
                    if copy_label(ui, &shown, if shown.len() > 26 { 11.5 } else { 13.0 }) {
                        self.copy(&ctx, &real);
                    }
                }
                ui.spacing_mut().item_spacing.y = sp::S;

                if !n.is_self
                    && let Some(m) = &self.matrix
                {
                    widgets::divider(ui, sp::M);
                    widgets::section(ui, "Access");
                    access_row(
                        ui,
                        "You can reach it",
                        &m.get(&s.me.stable_id, &n.stable_id),
                    );
                    access_row(
                        ui,
                        "It can reach you",
                        &m.get(&n.stable_id, &s.me.stable_id),
                    );
                }

                if !n.is_self && n.exit_node_option {
                    widgets::divider(ui, sp::M);
                    widgets::section(ui, "Exit node");
                    let using = s.prefs.exit_node_id == n.stable_id || n.exit_node_active;
                    let id = n.stable_id.clone();
                    if using {
                        ui.label(
                            RichText::new("Your internet traffic goes through this device.")
                                .size(ty::SMALL)
                                .color(GREEN),
                        );
                        if ui.add(Btn::new("Stop using it")).clicked() {
                            self.act(&ctx, "exit", || {
                                ts::set_exit_node(None).map(|_| "Exit node off".into())
                            });
                        }
                    } else if ui
                        .add_enabled(
                            n.online,
                            Btn::new("Route internet through it").glyph(icon::GLOBE),
                        )
                        .clicked()
                    {
                        self.act(&ctx, "exit", move || {
                            ts::set_exit_node(Some(&id)).map(|_| "Exit node set".into())
                        });
                    }
                }

                widgets::divider(ui, sp::M);
                widgets::section(ui, "Details");
                let (conn, col) = connection(&n);
                let os = self.os_of(&n);
                let owner = if n.tags.is_empty() {
                    let who = if n.user_display.is_empty() {
                        &n.user_login
                    } else {
                        &n.user_display
                    };
                    mask.who(who)
                } else {
                    "tagged device".into()
                };
                let mut rows: Vec<(&str, String, Color32)> = vec![
                    ("Owner", owner, TEXT),
                    ("Connection", conn, col),
                    ("System", os.title(), TEXT),
                ];
                if !n.tags.is_empty() {
                    rows.push(("Tags", n.tags.join(", "), PURPLE));
                }
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
                // The name the device gives itself, only when it differs from the
                // name the tailnet uses.
                if !same_name(&n.hostname, &n.name) && !n.hostname.is_empty() {
                    rows.push(("Device's own name", mask.hostname(&n), TEXT));
                }
                if !n.cur_addr.is_empty() {
                    rows.push(("Endpoint", mask.secret(&n.cur_addr), TEXT));
                }
                if !n.relay.is_empty() {
                    rows.push(("Home relay", n.relay.clone(), TEXT));
                }
                if !n.is_self {
                    rows.push((
                        "Sent / received",
                        format!("{} / {}", bytes(n.tx), bytes(n.rx)),
                        TEXT,
                    ));
                }
                if !n.primary_routes.is_empty() {
                    rows.push((
                        "Subnet routes",
                        mask.text(&n.primary_routes.join(", ")),
                        TEXT,
                    ));
                }
                rows.push(("Added", local_time(n.created), TEXT));
                rows.push(("Key expires", local_time(n.key_expiry), TEXT));
                if n.is_self {
                    rows.push(("Tailscale", s.version.clone(), TEXT));
                }
                egui::Grid::new("details")
                    .num_columns(2)
                    .min_row_height(18.0)
                    .spacing(vec2(sp::M, 6.0))
                    .show(ui, |ui| {
                        for (k, v, c) in rows {
                            ui.label(RichText::new(k).size(ty::SMALL).color(MUTED));
                            ui.label(RichText::new(v).size(ty::SMALL).color(c));
                            ui.end_row();
                        }
                    });
                if !n.is_self && !self.api.connected {
                    ui.add_space(sp::S);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label(
                            RichText::new("Versions of other devices come from the Tailscale API.")
                                .size(ty::SMALL)
                                .color(FAINT),
                        );
                        if ui.link(RichText::new("Connect").size(ty::SMALL)).clicked() {
                            self.onboard = Some(Onboarding::at(onboard::OS_STEP));
                        }
                    });
                }
                ui.add_space(sp::L);
            });
    }

    /// Beside the graph: who the focused device can reach and who can reach it.
    pub(super) fn graph_detail(&mut self, ui: &mut Ui) {
        let s = self.snap.clone().unwrap();
        let Some(n) = self.selected.as_ref().and_then(|id| s.node(id)).cloned() else {
            return;
        };
        let Some(m) = self.matrix.as_ref() else {
            return;
        };
        let mut groups: [Vec<Node>; 5] = Default::default();
        for o in s.all().filter(|o| o.stable_id != n.stable_id) {
            let out = m.get(&n.stable_id, &o.stable_id).allowed();
            let inn = m.get(&o.stable_id, &n.stable_id).allowed();
            // Only what is known both ways is stated; half-known pairs are unknown.
            let g = match (out, inn) {
                (Some(true), Some(true)) => 0,
                (Some(true), Some(false)) => 1,
                (Some(false), Some(true)) => 2,
                (Some(false), Some(false)) => 3,
                _ => 4,
            };
            groups[g].push(o.clone());
        }
        for g in &mut groups {
            g.sort_by_key(|o| super::display_order(o, &self.mask));
        }
        let reaches = groups[0].len() + groups[1].len();
        let reached = groups[0].len() + groups[2].len();
        let total = s.all().count() - 1;
        let sub = format!("Reaches {reaches} of {total}  ·  reached by {reached}");
        self.panel_header(ui, &n, &sub);
        ui.add_space(sp::M);
        if ui
            .add(Btn::new("Show in device list").glyph(icon::LIST))
            .clicked()
        {
            self.view = View::Devices;
        }
        let name = self.mask.name(&n);
        let titles = [
            ("Both ways".to_owned(), icon::ARROWS_H, TEXT),
            (format!("Only {name} can reach"), icon::ARROW_RIGHT, AMBER),
            (
                format!("Only they can reach {name}"),
                icon::ARROW_LEFT,
                AMBER,
            ),
            ("Blocked".to_owned(), icon::BAN, RED),
            ("Not known yet".to_owned(), icon::HELP, MUTED),
        ];
        let mut focus = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (i, (title, glyph, col)) in titles.iter().enumerate() {
                    if groups[i].is_empty() {
                        continue;
                    }
                    widgets::divider(ui, sp::M);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(14.0, 16.0), Sense::hover());
                        paint_glyph(ui.painter(), r.center(), glyph, 11.0, *col);
                        ui.label(
                            RichText::new(format!("{title}  {}", groups[i].len()))
                                .size(ty::SMALL)
                                .color(MUTED)
                                .strong(),
                        );
                    });
                    ui.add_space(sp::XS);
                    for o in &groups[i] {
                        if self.peer_row(ui, o).clicked() {
                            focus = Some(o.stable_id.clone());
                        }
                    }
                }
                ui.add_space(sp::L);
            });
        if let Some(f) = focus {
            self.selected = Some(f);
        }
    }

    /// A device in a list: small badge and name, the whole row clickable.
    fn peer_row(&self, ui: &mut Ui, o: &Node) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, RADIUS, HOVER);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let p = ui.painter();
        let c = rect.left_center() + vec2(sp::S + 10.0, 0.0);
        paint_badge(p, c, 10.0, self.icon_of(o).glyph, Badge::of(o), 1.0);
        let col = if o.online || o.is_self { TEXT } else { MUTED };
        widgets::paint_text(
            p,
            c + vec2(10.0 + sp::M, 0.0),
            &self.mask.name(o),
            ty::BUTTON,
            col,
        );
        resp
    }
}

/// One direction of access: a mark, what it means, and how it is known.
fn access_row(ui: &mut Ui, what: &str, a: &Access) {
    let (glyph, col, how) = access_text(a);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
        if glyph == "?" {
            paint_glyph(ui.painter(), r.center(), icon::HELP, 12.0, col);
        } else {
            paint_glyph(ui.painter(), r.center(), glyph, 12.0, col);
        }
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(RichText::new(what).size(ty::BUTTON));
            ui.label(RichText::new(how).size(ty::SMALL).color(MUTED));
        });
    });
}
