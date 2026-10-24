#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod audio_view;
mod database_view;
mod docs_view;
mod files_view;
mod index_view;
mod player;
mod settings;
mod table_columns;
mod texture_view;
mod theme;
mod thumbs;

/// The window icon: an eye drawn at 64x64.
fn icon() -> egui::IconData {
    let n = 64usize;
    let mut rgba = vec![0u8; n * n * 4];
    let c = (n as f32 - 1.0) / 2.0;
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (x as f32 - c, y as f32 - c);
            // almond shape: inside two circles offset vertically
            let r = 30.0;
            let inside_eye =
                (dx * dx + (dy - 19.0) * (dy - 19.0)).sqrt() < r && (dx * dx + (dy + 19.0) * (dy + 19.0)).sqrt() < r;
            let d = (dx * dx + dy * dy).sqrt();
            let px = &mut rgba[(y * n + x) * 4..][..4];
            if inside_eye {
                if d < 6.0 {
                    px.copy_from_slice(&[theme::ICON_DARK[0], theme::ICON_DARK[1], theme::ICON_DARK[2], 255]);
                } else if d < 14.0 {
                    px.copy_from_slice(&[theme::ICON_ACCENT[0], theme::ICON_ACCENT[1], theme::ICON_ACCENT[2], 255]);
                } else {
                    px.copy_from_slice(&[theme::ICON_LIGHT[0], theme::ICON_LIGHT[1], theme::ICON_LIGHT[2], 255]);
                }
            }
        }
    }
    egui::IconData { rgba, width: n as u32, height: n as u32 }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!(
                "Eyefinder — {}",
                &env!("EYEFINDER_REVISION")[..7.min(env!("EYEFINDER_REVISION").len())]
            ))
            .with_app_id("eyefinder")
            .with_inner_size([1320.0, 820.0])
            .with_min_inner_size([960.0, 600.0])
            .with_icon(icon())
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("Eyefinder", options, Box::new(|cc| Ok(Box::new(app::EyefinderApp::new(cc)))))
}
