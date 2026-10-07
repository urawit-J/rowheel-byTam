mod app;
mod calibration;
mod config;
mod force_feedback;
mod input;
mod virtual_controller;

use app::RoWheelApp;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // Tall enough for a whole calibration step without scrolling:
            // heading, progress, instructions, the wheel photo, the detected
            // box and the step controls. Smaller still works -- the controls
            // have a panel of their own and the rest scrolls.
            .with_inner_size([700.0, 820.0])
            .with_min_inner_size([400.0, 320.0])
            .with_title("RoWheel"),
        ..Default::default()
    };

    eframe::run_native(
        "RoWheel",
        native_options,
        Box::new(|cc| Ok(Box::new(RoWheelApp::new(cc)))),
    )
}
