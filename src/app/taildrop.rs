//! Taildrop, built around files: one big place to drop them, the devices to send
//! them to as chips under it, and a small inbox for what other devices sent here.
//! Files and devices can be picked in either order; nothing leaves until Send.

use std::path::PathBuf;

use eframe::egui::{self, Align, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, vec2};

use super::{App, Msg, bytes, files_label, icon_button, send_all};
use crate::theme::{icon, *};
use crate::ts::{self, Node};
use crate::widgets::{self, Btn};

/// What the Taildrop screen remembers between frames.
#[derive(Default)]
pub(super) struct DropState {
    /// Files waiting for Send, with their sizes.
    pub files: Vec<(PathBuf, u64)>,
    /// Devices picked as targets, by stable id.
    pub targets: std::collections::HashSet<String>,
    /// Test hook: device names to pick once the first snapshot is in.
    pub pending_names: Vec<String>,
    pub show_unavailable: bool,
    /// Where each device chip was drawn last frame, so a file dropped on one
    /// picks that device too.
    pub chips: Vec<(Rect, String)>,
    /// The pointer when a file drag came over the window. Some systems don't
    /// move the pointer during a drag, so a drop only counts as onto a chip
    /// when the pointer moved since.
    pub drag_from: Option<Option<Pos2>>,
    /// How many devices the running send goes to; 0 when none is running.
    pub sending: usize,
}

impl App {
    /// A send finished. Devices that got everything are unpicked, so a retry
    /// only goes to the ones that failed; when all made it, the sent files
    /// leave the list. Files added during the send stay.
    pub(super) fn sent(&mut self, files: Vec<PathBuf>, delivered: Vec<String>) {
        let all = delivered.len() == self.drop.sending;
        for id in &delivered {
            self.drop.targets.remove(id);
        }
        if all {
            self.drop.files.retain(|(f, _)| !files.contains(f));
        }
        self.drop.sending = 0;
    }

    /// Adds files to the list waiting for Send, skipping ones already on it.
    pub(super) fn stage(&mut self, files: Vec<PathBuf>) {
        for f in files {
            if self.drop.files.iter().any(|(p, _)| *p == f) {
                continue;
            }
            let size = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
            self.drop.files.push((f, size));
        }
    }

    pub(super) fn taildrop(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let s = self.snap.clone().unwrap();
        let mut peers: Vec<&Node> = s.peers.iter().collect();
        peers.sort_by_key(|p| super::display_order(p, &self.mask));
        let (ready, unavailable): (Vec<&Node>, Vec<&Node>) =
            peers.into_iter().partition(|p| p.taildrop_ok());
        // Names that don't match yet wait for a later snapshot.
        let names = std::mem::take(&mut self.drop.pending_names);
        for name in names {
            match ready.iter().find(|p| self.mask.name(p) == name) {
                Some(p) => {
                    self.drop.targets.insert(p.stable_id.clone());
                }
                None => self.drop.pending_names.push(name),
            }
        }
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
                    self.drop_zone(ui, &ctx);
                    if !self.drop.files.is_empty() {
                        ui.add_space(sp::M);
                        self.staged(ui);
                    }
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
                                "Picked. Click to leave out"
                            } else {
                                "Click to pick. Pick as many as you like"
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

                    ui.add_space(sp::L);
                    self.send_bar(ui, &ctx, &picked);
                    ui.add_space(sp::XL);
                    self.inbox(ui, &ctx, &s.waiting_files);
                    ui.add_space(sp::XL);
            });
        });
    }

    /// The hero: a dashed box that takes dropped files, with a button for
    /// people who would rather pick them. Smaller once files are waiting.
    fn drop_zone(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let dragging = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let compact = !self.drop.files.is_empty();
        let h = if compact { 120.0 } else { 200.0 };
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
        let p = ui.painter();
        let (stroke, fill) = if dragging {
            (Stroke::new(1.5, ACCENT), ACCENT.gamma_multiply(0.08))
        } else {
            (Stroke::new(1.2, EDGE), SURFACE)
        };
        p.rect_filled(rect, RADIUS, fill);
        dashed_round_rect(p, rect, RADIUS, stroke);

        let c = rect.center();
        let title = match (dragging, compact) {
            (true, _) => "Let go to add them",
            (false, true) => "Drop more files here",
            (false, false) => "Drop files here",
        };
        let font = egui::FontId::new(17.0, egui::FontFamily::Name("strong".into()));
        let (title_at, button_at) = if compact {
            (c - vec2(0.0, 18.0), c + vec2(0.0, 22.0))
        } else {
            let at = c - vec2(0.0, 44.0);
            p.circle_filled(at, 22.0, if dragging { ACCENT.gamma_multiply(0.2) } else { HOVER });
            paint_glyph(p, at, icon::UPLOAD, 18.0, if dragging { ACCENT } else { TEXT });
            p.text(
                c + vec2(0.0, 18.0),
                egui::Align2::CENTER_CENTER,
                "Then pick who gets them and press Send",
                egui::FontId::proportional(ty::BUTTON),
                MUTED,
            );
            (c - vec2(0.0, 6.0), c + vec2(0.0, 58.0))
        };
        p.text(title_at, egui::Align2::CENTER_CENTER, title, font, TEXT);
        let busy = self.busy.contains("pick");
        let label = if compact { "Add files" } else { "Choose files" };
        let br = Rect::from_center_size(button_at, vec2(130.0, 32.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(br)
                .layout(Layout::top_down(Align::Center)),
        );
        let mut b = Btn::new(label).glyph(icon::UPLOAD);
        if !compact {
            b = b.primary();
        }
        if child.add_enabled(!busy, b).clicked() {
            self.pick_files(ctx);
        }
    }

    /// The files waiting for Send, each with a way to take it back out.
    fn staged(&mut self, ui: &mut Ui) {
        // The running send already has its copy of the list.
        let busy = self.busy.contains("send-many");
        ui.horizontal(|ui| {
            ui.set_min_height(28.0);
            let n = self.drop.files.len();
            let total: u64 = self.drop.files.iter().map(|(_, b)| b).sum();
            widgets::section(ui, &format!("Files to send ({n}, {})", bytes(total)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled(!busy, Btn::new("Clear").small()).clicked() {
                    self.drop.files.clear();
                }
            });
        });
        let mut remove = None;
        for (i, (f, size)) in self.drop.files.iter().enumerate() {
            if i > 0 {
                widgets::divider(ui, 0.0);
            }
            ui.horizontal(|ui| {
                ui.set_min_height(34.0);
                let name = f.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
                ui.label(RichText::new(name).monospace())
                    .on_hover_text(f.display().to_string());
                ui.label(RichText::new(bytes(*size)).size(ty::SMALL).color(MUTED));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(ui, icon::CLOSE, "Take out", !busy) {
                        remove = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove {
            self.drop.files.remove(i);
        }
    }

    /// The one button that sends, saying what goes where, or what is missing.
    fn send_bar(&mut self, ui: &mut Ui, ctx: &egui::Context, picked: &[Node]) {
        let n = self.drop.files.len();
        let names: Vec<String> = picked.iter().map(|p| self.mask.name(p)).collect();
        let what = files_label(n);
        let to = match names.len() {
            0 => String::new(),
            1 => names[0].clone(),
            2 => format!("{} and {}", names[0], names[1]),
            k => format!("{k} devices"),
        };
        let busy = self.busy.contains("send-many");
        let (hint, ready) = match (n, names.len()) {
            _ if busy => ("Sending...".to_owned(), false),
            (0, 0) => ("Add files and pick devices, in any order".to_owned(), false),
            (0, _) => (format!("Add files to send to {to}"), false),
            (_, 0) => (format!("Pick who gets the {what}"), false),
            _ => (format!("{what} to {to}"), true),
        };
        ui.horizontal(|ui| {
            ui.set_min_height(36.0);
            if busy {
                ui.add(egui::Spinner::new());
            }
            ui.label(RichText::new(hint).size(ty::BUTTON).color(if ready { TEXT } else { MUTED }));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let b = Btn::new("Send").glyph(icon::SEND).primary();
                if ui.add_enabled(ready, b).clicked() {
                    self.send_staged(ctx, picked);
                }
            });
        });
    }

    fn send_staged(&mut self, ctx: &egui::Context, picked: &[Node]) {
        let files: Vec<PathBuf> = self.drop.files.iter().map(|(f, _)| f.clone()).collect();
        let targets: Vec<Target> = picked
            .iter()
            .map(|n| (n.stable_id.clone(), n.ipv4().to_owned(), self.mask.name(n)))
            .collect();
        self.drop.sending = targets.len();
        let tx = self.tx.clone();
        self.act(ctx, "send-many", move || {
            let (delivered, r) = send_to_all(&files, &targets);
            tx.send(Msg::Sent(files, delivered)).ok();
            r
        });
    }

    /// Opens the file dialog off the UI thread; the choice comes back as `Msg::Picked`.
    fn pick_files(&mut self, ctx: &egui::Context) {
        let tx = self.tx.clone();
        self.act(ctx, "pick", move || {
            if let Some(files) = rfd::FileDialog::new().set_title("Choose files to send").pick_files() {
                tx.send(Msg::Picked(files)).ok();
            }
            Ok(String::new())
        });
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

    /// Files dropped on the Taildrop screen join the list. Dropped onto a
    /// chip, they also pick that device. Nothing is sent until Send.
    pub(super) fn drop_on_taildrop(&mut self, ctx: &egui::Context, files: Vec<PathBuf>) {
        let pos = ctx.input(|i| i.pointer.latest_pos());
        let moved = self.drop.drag_from.take().is_some_and(|from| from != pos);
        let on_chip = pos.filter(|_| moved).and_then(|p| {
            self.drop
                .chips
                .iter()
                .find(|(r, _)| r.contains(p))
                .map(|(_, id)| id.clone())
        });
        if let Some(id) = on_chip {
            self.drop.targets.insert(id);
        }
        self.stage(files);
    }
}

/// A device to send to: stable id, address, shown name.
type Target = (String, String, String);

/// Tries every device even when one fails, and says which got the files.
/// Also returns the ids of the devices that got all of them.
fn send_to_all(files: &[PathBuf], targets: &[Target]) -> (Vec<String>, Result<String, String>) {
    let (mut ids, mut ok, mut failed) = (Vec::new(), Vec::new(), Vec::new());
    for (id, ip, name) in targets {
        match send_all(files, ip, name) {
            Ok(_) => {
                ids.push(id.clone());
                ok.push(name.clone());
            }
            Err(e) => failed.push(format!("{name}: {e}")),
        }
    }
    let what = files_label(files.len());
    let r = match (ok.is_empty(), failed.is_empty()) {
        (_, true) => Ok(format!("Sent {what} to {}", ok.join(", "))),
        (true, false) => Err(format!("Could not send {what}. {}", failed.join("; "))),
        (false, false) => Err(format!(
            "Sent {what} to {}, but not to {}",
            ok.join(", "),
            failed.join("; ")
        )),
    };
    (ids, r)
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

