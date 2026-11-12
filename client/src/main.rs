#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod appearance;
mod artwork;
mod avatars;
mod model;
mod motion;
mod network;
mod packs;
mod survivability;

fn main() -> eframe::Result {
    let (layout, layout_warning) = appearance::Layout::load();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size(layout.window_size)
            .with_min_inner_size([820.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Murr Bunker",
        options,
        Box::new(move |cc| Ok(Box::new(app::BunkerApp::new(cc, layout, layout_warning)))),
    )
}
