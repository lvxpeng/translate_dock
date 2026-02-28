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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([432.0, 332.0])
            .with_position([1230.0, 650.0])
            .with_icon(load_window_icon()) // 设置窗口图标，解决任务管理器中显示 egui 默认图标的问题
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
