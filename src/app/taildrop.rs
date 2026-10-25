//! Taildrop, built around files: one big place to drop them, the devices to send
//! them to as chips under it, and a small inbox for what other devices sent here.

use std::path::PathBuf;

use eframe::egui::{self, Align, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, vec2};

use super::{App, bytes, icon_button, send_all};
use crate::theme::{icon, *};
use crate::ts::{self, Node};
use crate::widgets::{self, Btn};

/// What the Taildrop screen remembers between frames.
#[derive(Default)]
pub(super) struct DropState {
    /// Devices picked as targets, by stable id.
    pub targets: std::collections::HashSet<String>,
    pub show_unavailable: bool,
    /// Where each device chip was drawn last frame, so a file dropped on one
    /// goes straight to that device.
    pub chips: Vec<(Rect, String)>,
    /// The pointer when a file drag came over the window. Some systems don't
    /// move the pointer during a drag, so a drop only counts as onto a chip
    /// when the pointer moved since.
    pub drag_from: Option<Option<Pos2>>,
}

impl App {
    pub(super) fn taildrop(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let s = self.snap.clone().unwrap();
        let mut peers: Vec<&Node> = s.peers.iter().collect();
        peers.sort_by_key(|p| super::display_order(p, &self.mask));
        let (ready, unavailable): (Vec<&Node>, Vec<&Node>) =
            peers.into_iter().partition(|p| p.taildrop_ok());
        // A device that went away can't stay picked.
        self.drop.targets.retain(|id| ready.iter().any(|p| &p.stable_id == id));
        self.drop.chips.clear();
        let dragging = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if dragging && self.drop.drag_from.is_none() {
            self.drop.drag_from = Some(ctx.input(|i| i.pointer.latest_pos()));
        }
        let picked: Vec<Node> = ready
            .iter()
            .filter(|p| self.drop.targets.contains(&p.stable_id))
            .map(|p| (*p).clone())
            .collect();

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            widgets::column(ui, widgets::COLUMN, |ui| {
                    widgets::page_title(ui, "Taildrop", "Send files between your devices", |_| {});
                    self.drop_zone(ui, &ctx, &picked);
                    ui.add_space(sp::XL);

                    widgets::section(ui, "Send to");
                    ui.add_space(sp::XS);
                    if ready.is_empty() {
                        ui.label(
                            RichText::new("No device can take files right now.")
                                .size(ty::BUTTON)
                                .color(MUTED),
                        );
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(sp::S, sp::S);
                        for p in &ready {
                            let on = self.drop.targets.contains(&p.stable_id);
                            let r = self.target_chip(ui, p, on, true);
                            if r.clicked() {
                                if on {
                                    self.drop.targets.remove(&p.stable_id);
                                } else {
                                    self.drop.targets.insert(p.stable_id.clone());
                                }
                            }
                            let tip = if on {
                                "Picked. Click to leave out, or drop files right on it"
                            } else {
                                "Click to pick, or drop files right on it"
                            };
                            r.on_hover_text(tip);
                        }
                    });
                    if !unavailable.is_empty() {
                        ui.add_space(sp::XS);
                        let open = self.drop.show_unavailable;
                        let label = format!("Unavailable ({})", unavailable.len());
                        let glyph = if open {
                            icon::CHEVRON_DOWN
                        } else {
                            icon::CHEVRON_RIGHT
                        };
                        if disclosure(ui, glyph, &label).clicked() {
                            self.drop.show_unavailable = !open;
                        }
                        if open {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = vec2(sp::S, sp::S);
                                for p in &unavailable {
                                    self.target_chip(ui, p, false, false)
                                        .on_hover_text(p.taildrop_reason());
                                }
                            });
                        }
                    }

                    ui.add_space(sp::XL);
                    self.inbox(ui, &ctx, &s.waiting_files);
                    ui.add_space(sp::XL);
            });
        });
    }

    /// The hero: a dashed box that takes dropped files, with a button for
    /// people who would rather pick them.
    fn drop_zone(&mut self, ui: &mut Ui, ctx: &egui::Context, picked: &[Node]) {
        let dragging = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 200.0), Sense::hover());
        let p = ui.painter();
        let (stroke, fill) = if dragging {
            (Stroke::new(1.5, ACCENT), ACCENT.gamma_multiply(0.08))
        } else {
            (Stroke::new(1.2, EDGE), SURFACE)
        };
        p.rect_filled(rect, RADIUS, fill);
        dashed_round_rect(p, rect, RADIUS, stroke);

        let c = rect.center();
        p.circle_filled(c - vec2(0.0, 44.0), 22.0, if dragging { ACCENT.gamma_multiply(0.2) } else { HOVER });
        paint_glyph(p, c - vec2(0.0, 44.0), icon::UPLOAD, 18.0, if dragging { ACCENT } else { TEXT });
        let title = if dragging {
            "Let go to send"
        } else {
            "Drop files here"
        };
        p.text(
            c - vec2(0.0, 6.0),
            egui::Align2::CENTER_CENTER,
            title,
            egui::FontId::new(17.0, egui::FontFamily::Name("strong".into())),
            TEXT,
        );
        let names: Vec<String> = picked.iter().map(|n| self.mask.name(n)).collect();
        let hint = match names.len() {
            0 => "Pick devices below, or drop files straight onto one".to_owned(),
            1 => format!("Sends to {}", names[0]),
            2 => format!("Sends to {} and {}", names[0], names[1]),
            n => format!("Sends to {} devices", n),
        };
        p.text(
            c + vec2(0.0, 18.0),
            egui::Align2::CENTER_CENTER,
            hint,
            egui::FontId::proportional(ty::BUTTON),
            MUTED,
        );
        let busy = self.busy.contains("send-many");
        let label = "Choose files";
        let bw = 130.0;
        let br = Rect::from_center_size(c + vec2(0.0, 58.0), vec2(bw, 32.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(br)
                .layout(Layout::top_down(Align::Center)),
        );
        let b = child
            .add_enabled(!busy, Btn::new(label).glyph(icon::SEND).primary())
            .on_disabled_hover_text("Sending");
        if b.clicked() {
            if picked.is_empty() {
                self.toast("Pick a device to send to first", true);
            } else {
                self.pick_and_send_many(ctx, picked.to_vec());
            }
        }
    }

    /// A device as a target: its badge and name in a chip that can be picked.
    fn target_chip(&mut self, ui: &mut Ui, n: &Node, on: bool, enabled: bool) -> egui::Response {
        let name = self.mask.name(n);
        let g = ui.painter().layout_no_wrap(
            name,
            egui::FontId::proportional(ty::BUTTON),
            TEXT,
        );
        let w = 6.0 + 20.0 + sp::S + g.size().x + if on { sp::S + 12.0 } else { 0.0 } + sp::M;
        let sense = if enabled { Sense::click() } else { Sense::hover() };
        let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), sense);
        if enabled {
            self.drop.chips.push((rect, n.stable_id.clone()));
        }
        let dragging = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let over = dragging
            && enabled
            && ui
                .ctx()
                .input(|i| i.pointer.latest_pos())
                .is_some_and(|p| rect.contains(p));
        let fill = if over {
            ACCENT.gamma_multiply(0.3)
        } else if on {
            SELECTED
        } else if resp.hovered() && enabled {
            PRESSED
        } else {
            HOVER
        };
        let a = if enabled { 1.0 } else { 0.45 };
        let p = ui.painter();
        p.rect_filled(rect, RADIUS, fill.gamma_multiply(a));
        if on || over {
            p.rect_stroke(rect, RADIUS, Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
        }
        let c = rect.left_center() + vec2(6.0 + 10.0, 0.0);
        paint_badge(p, c, 10.0, self.icon_of(n).glyph, crate::theme::Badge::of(n), a);
        let x = c.x + 10.0 + sp::S;
        let gw = g.size().x;
        widgets::text_at_mid(p, egui::pos2(x, rect.center().y), g, TEXT.gamma_multiply(a));
        if on {
            paint_glyph(p, egui::pos2(x + gw + sp::S + 6.0, rect.center().y), icon::CHECK, 11.0, ACCENT);
        }
        if resp.hovered() && enabled {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp
    }

    /// Files other devices sent here. Quiet when empty.
    fn inbox(&mut self, ui: &mut Ui, ctx: &egui::Context, files: &[ts::WaitingFile]) {
        ui.horizontal(|ui| {
            ui.set_min_height(28.0);
            widgets::section(ui, if files.is_empty() { "Incoming" } else { "Incoming files" });
            if files.is_empty() {
                return;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let busy = self.busy.contains("receive");
                let b = Btn::new("Save all to Downloads").glyph(icon::DOWNLOAD).small();
                if ui.add_enabled(!busy, b).clicked() {
                    self.act(ctx, "receive", || {
                        ts::receive_files().map(|d| format!("Saved to {}", d.display()))
                    });
                }
                if busy {
                    ui.add(egui::Spinner::new());
                }
            });
        });
        if files.is_empty() {
            ui.label(
                RichText::new("Nothing waiting. Files sent to this device land here.")
                    .size(ty::BUTTON)
                    .color(FAINT),
            );
            return;
        }
        ui.add_space(sp::XS);
        for (i, f) in files.iter().enumerate() {
            if i > 0 {
                widgets::divider(ui, 0.0);
            }
            ui.horizontal(|ui| {
                ui.set_min_height(36.0);
                ui.label(RichText::new(&f.name).monospace());
                ui.label(RichText::new(bytes(f.size)).size(ty::SMALL).color(MUTED));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(ui, icon::TRASH, "Discard", true) {
                        let name = f.name.clone();
                        self.act(ctx, &format!("del-{name}"), move || {
                            ts::delete_waiting_file(&name).map(|_| format!("Discarded {name}"))
                        });
                    }
                });
            });
        }
    }

    /// One file dialog, the chosen files sent to every picked device.
    fn pick_and_send_many(&mut self, ctx: &egui::Context, nodes: Vec<Node>) {
        let targets: Vec<(String, String)> = nodes
            .iter()
            .map(|n| (n.ipv4().to_owned(), self.mask.name(n)))
            .collect();
        self.act(ctx, "send-many", move || {
            let Some(files) = rfd::FileDialog::new().set_title("Send files").pick_files() else {
                return Ok(String::new());
            };
            send_to_all(&files, &targets)
        });
    }

    /// Files dropped on the Taildrop screen: onto a chip goes to that device,
    /// anywhere else to every picked device.
    pub(super) fn drop_on_taildrop(&mut self, ctx: &egui::Context, files: Vec<PathBuf>) {
        let s = match &self.snap {
            Some(s) => s.clone(),
            None => return,
        };
        let pos = ctx.input(|i| i.pointer.latest_pos());
        let moved = self.drop.drag_from.take().is_some_and(|from| from != pos);
        let on_chip = pos.filter(|_| moved).and_then(|p| {
            self.drop
                .chips
                .iter()
                .find(|(r, _)| r.contains(p))
                .map(|(_, id)| id.clone())
        });
        let mut ids: Vec<String> = match on_chip {
            Some(id) => vec![id],
            None => self.drop.targets.iter().cloned().collect(),
        };
        ids.sort();
        let targets: Vec<(String, String)> = ids
            .iter()
            .filter_map(|id| s.node(id))
            .filter(|n| n.taildrop_ok())
            .map(|n| (n.ipv4().to_owned(), self.mask.name(n)))
            .collect();
        if targets.is_empty() {
            self.toast("Pick a device first, or drop the files onto one", true);
            return;
        }
        if self.busy.contains("send-many") {
            self.toast("Still sending the last files; drop these again when it's done", true);
            return;
        }
        self.act(ctx, "send-many", move || send_to_all(&files, &targets));
    }
}

/// Tries every device even when one fails, and says which got the files.
fn send_to_all(files: &[PathBuf], targets: &[(String, String)]) -> Result<String, String> {
    let (mut ok, mut failed) = (Vec::new(), Vec::new());
    for (ip, name) in targets {
        match send_all(files, ip, name) {
            Ok(_) => ok.push(name.clone()),
            Err(e) => failed.push(format!("{name}: {e}")),
        }
    }
    let n = files.len();
    let what = format!("{n} file{}", if n == 1 { "" } else { "s" });
    match (ok.is_empty(), failed.is_empty()) {
        (_, true) => Ok(format!("Sent {what} to {}", ok.join(", "))),
        (true, false) => Err(format!("Could not send {what}. {}", failed.join("; "))),
        (false, false) => Err(format!(
            "Sent {what} to {}, but not to {}",
            ok.join(", "),
            failed.join("; ")
        )),
    }
}

/// A row that opens and closes a list, chevron first.
fn disclosure(ui: &mut Ui, glyph: &str, label: &str) -> egui::Response {
    let g = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(ty::BUTTON),
        MUTED,
    );
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 22.0, 28.0), Sense::click());
    let col = if resp.hovered() { TEXT } else { MUTED };
    paint_glyph(ui.painter(), rect.left_center() + vec2(6.0, 0.0), glyph, 9.0, col);
    widgets::text_at_mid(ui.painter(), rect.left_center() + vec2(18.0, 0.0), g, col);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// The outline of a rounded rectangle, dashed.
fn dashed_round_rect(p: &egui::Painter, r: Rect, radius: f32, stroke: Stroke) {
    let mut pts: Vec<Pos2> = Vec::new();
    let corners = [
        (r.right_top() + vec2(-radius, radius), -90.0f32),
        (r.right_bottom() + vec2(-radius, -radius), 0.0),
        (r.left_bottom() + vec2(radius, -radius), 90.0),
        (r.left_top() + vec2(radius, radius), 180.0),
    ];
    for (c, start) in corners {
        for k in 0..=6 {
            let a = (start + k as f32 * 15.0).to_radians();
            pts.push(c + vec2(a.cos(), a.sin()) * radius);
        }
    }
    pts.push(pts[0]);
    p.extend(Shape::dashed_line(&pts, stroke, 7.0, 5.0));
}

