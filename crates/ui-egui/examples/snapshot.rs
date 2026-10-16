//! Renders the Pecunia window offscreen (wgpu, no window) to a PNG, for looking at UI changes.
//!
//! `cargo run -p pecunia-ui-egui --example snapshot -- [--sample] [--in file.pecunia]
//!  [--size 1440x900] [--scale 2] [--dark] [--cmd 'id={json}']... [--dialog 'name={json}']
//!  [--palette 'query'] [--no-sidebar] [--menu (the phone menu)] out.png`

use pecunia_engine::Session;
use pecunia_ui_egui::{PecuniaApp, ThemeMode};
use serde_json::json;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = None;
    let mut size = (1440.0f32, 900.0f32);
    let mut scale = 2.0f32;
    let mut cmds: Vec<String> = vec![];
    let mut dark = false;
    let mut dialog: Option<String> = None;
    let mut palette: Option<String> = None;
    let mut sidebar = true;
    let mut menu = false;
    let mut out = "snapshot.png".to_string();
    let mut sample = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--sample" => sample = true,
            "--in" => {
                i += 1;
                input = args.get(i).cloned();
            }
            "--size" => {
                i += 1;
                if let Some((w, h)) = args.get(i).and_then(|s| s.split_once('x')) {
                    size = (w.parse().unwrap_or(1440.0), h.parse().unwrap_or(900.0));
                }
            }
            "--scale" => {
                i += 1;
                scale = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(2.0);
            }
            "--cmd" => {
                i += 1;
                if let Some(c) = args.get(i) {
                    cmds.push(c.clone());
                }
            }
            "--dark" => dark = true,
            "--no-sidebar" => sidebar = false,
            "--menu" => menu = true,
            "--dialog" => {
                i += 1;
                dialog = args.get(i).cloned();
            }
            "--palette" => {
                i += 1;
                palette = args.get(i).cloned();
            }
            other => out = other.to_string(),
        }
        i += 1;
    }
    let mut session = match &input {
        Some(p) => Session::open(p).expect("open"),
        None if sample => Session::sample(),
        None => Session::scratch(),
    };
    let parse =
        |c: &String| c.split_once('=').map(|(a, b)| (a.to_string(), serde_json::from_str(b).unwrap_or(json!({})))).unwrap_or((c.clone(), json!({})));
    // Engine commands run first; UI-only commands (`ui.*`) once the app exists.
    for c in cmds.iter().filter(|c| !c.starts_with("ui.")) {
        let (id, p) = parse(c);
        if let Err(e) = session.execute(&id, p) {
            eprintln!("{id}: {e}");
        }
    }
    let mut app = PecuniaApp::new(session);
    for c in cmds.iter().filter(|c| c.starts_with("ui.")) {
        let (id, p) = parse(c);
        app.run_any(&id, p);
    }
    app.ui.theme = if dark { ThemeMode::Dark } else { ThemeMode::Light };
    app.ui.sidebar = sidebar;
    app.ui.menu = menu;
    app.route();
    app.ui.toasts.clear();
    if let Some(d) = &dialog {
        let (name, p) = d.split_once('=').map(|(a, b)| (a, serde_json::from_str(b).unwrap_or(json!({})))).unwrap_or((d.as_str(), json!({})));
        app.open_dialog(name, p);
    }
    if let Some(q) = palette {
        app.ui.palette = Some(pecunia_ui_egui::palette::Palette::with_query(&q));
    }
    let mut harness =
        egui_kittest::Harness::builder().with_size(egui::vec2(size.0, size.1)).with_pixels_per_point(scale).with_max_steps(12).wgpu().build_ui_state(
            move |ui, app: &mut PecuniaApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                // The harness adds an 8 pt outer margin; the real window has none.
                let full = ui.max_rect().expand(8.0);
                ui.scope_builder(egui::UiBuilder::new().max_rect(full), |ui| app.ui(ui));
            },
            app,
        );
    harness.run_steps(8);
    let img = harness.render().expect("render");
    img.save(&out).expect("save png");
    eprintln!("wrote {out} ({}x{})", img.width(), img.height());
}
