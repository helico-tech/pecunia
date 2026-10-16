//! Renders the app icon (drawn in `icons::app_icon`) to a 1024×1024 PNG with a transparent
//! background. `cargo xtask bundle` uses it for the macOS .icns.
//!
//! `cargo run -p pecunia-ui-egui --example app_icon -- out.png`

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "icon-1024.png".into());
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(512.0, 512.0)).with_pixels_per_point(2.0).wgpu().build_ui(|ui| {
        ui.ctx().global_style_mut(|s| s.visuals.panel_fill = egui::Color32::TRANSPARENT);
        let full = ui.max_rect().expand(8.0);
        pecunia_ui_egui::icons::app_icon(ui.painter(), full);
    });
    harness.run_steps(2);
    let img = harness.render().expect("render");
    img.save(&out).expect("save png");
    eprintln!("wrote {out} ({}x{})", img.width(), img.height());
}
