//! Ilmarfy para desktop (Mac e Linux/Fedora): o mesmo núcleo do app Android, com interface em egui.

mod app;
mod playback;

fn data_dir() -> std::path::PathBuf {
    std::env::var_os("ILMARFY_DESKTOP_DATA")
        .map(Into::into)
        .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(|| ".".into()).join("Ilmarfy"))
}

fn main() -> eframe::Result {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).ok();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("Ilmarfy")
        .with_app_id("ilmarfy")
        .with_inner_size([1200.0, 760.0])
        .with_min_inner_size([820.0, 560.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let dir = data_dir();
    eframe::run_native("Ilmarfy", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, dir)))))
}
