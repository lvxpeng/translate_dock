#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod translate;

use eframe::egui;

fn load_window_icon() -> egui::IconData {
    let ico_bytes = include_bytes!("assets/icons/ico.ico");
    let img = image::load_from_memory(ico_bytes)
        .expect("failed to decode ico")
        .into_rgba8();
    let (width, height) = img.dimensions();
    egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    }
}

#[tokio::main]
async fn main() -> Result<(), eframe::Error> {
    env_logger::init();

    // 动态计算窗口位置：右下角，紧贴任务栏上方
    let window_size = [432.0, 382.0];
    let total_width = window_size[0] + 32.0;   // + outer_margin * 2
    let total_height = window_size[1] + 12.0;
    let pos = app::calculate_initial_position(total_width, total_height);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(window_size)
            .with_position(pos)
            .with_icon(load_window_icon())
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top(),
        ..Default::default()
    };
    eframe::run_native(
        "Translate Dock",
        options,
        Box::new(|cc| Ok(Box::new(app::TranslateApp::new(cc)))),
    )
}
