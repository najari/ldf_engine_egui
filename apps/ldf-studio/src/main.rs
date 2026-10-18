//! LDF Studio 진입점.

mod app;
mod fonts;
mod ui;
mod view;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("LDF Studio")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 600.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "LDF Studio",
        options,
        Box::new(|cc| Ok(Box::new(app::StudioApp::new(cc)))),
    )
}
