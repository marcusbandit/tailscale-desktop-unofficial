mod access;
mod app;
mod graph;
mod osinfo;
mod shot;
mod theme;
mod ts;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Tailscale")
            .with_app_id("tailscale-desktop-unofficial")
            .with_inner_size([1320.0, 820.0])
            .with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Tailscale",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
