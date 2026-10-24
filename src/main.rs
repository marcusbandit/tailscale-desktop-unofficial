mod access;
mod app;
mod graph;
mod osinfo;
mod shot;
mod theme;
mod ts;

/// Shown wherever the app names itself. It is not made by Tailscale, and says so.
pub const APP_NAME: &str = "Tailscale Desktop (unofficial)";

fn main() -> eframe::Result {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title(APP_NAME)
        .with_app_id("tailscale-desktop-unofficial")
        .with_inner_size([1320.0, 820.0])
        .with_min_inner_size([760.0, 480.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
