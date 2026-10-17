//! `cargo xtask ci | layers | bundle`.
//!
//! - `ci`: fmt check, clippy `-D warnings`, tests, layers. Run before every commit.
//! - `layers`: each crate may depend only on lower layers; nothing below the UI uses a UI crate.
//! - `bundle [--universal] [--dmg]`: builds `target/release/bundle/Pecunia.app` (macOS); with
//!   `--universal` for both Apple Silicon and Intel; with `--dmg` also a disk image and a CLI
//!   archive in `target/dist/`.
//! - `package-linux [--target TRIPLE]`: builds `target/dist/pecunia-<version>-linux-<arch>.tar.gz`.
//! - `package-windows`: builds `target/dist/pecunia-<version>-windows-<arch>.zip`.
//! - `web`: builds the browser app into `target/web/` (needs the wasm32 target and a matching
//!   `wasm-bindgen` CLI and `wasm-opt` from binaryen). Deploy: see `apps/pecunia-web/wrangler.toml`.
//! - `icon`: re-renders the committed icon files in `packaging/` from `icons::app_icon`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde_json::Value;

/// Workspace crates and their layer. A crate may depend on crates in strictly lower layers.
const LAYERS: &[(&str, u8)] = &[
    ("pecunia-model", 0),
    ("pecunia-formats", 1),
    ("pecunia-store", 1),
    ("pecunia-engine", 2),
    ("pecunia-mcp", 3),
    ("pecunia-ui-egui", 4),
    ("pecunia", 5),
    ("pecunia-cli", 5),
    ("pecunia-web", 5),
];

/// UI and windowing crates only the UI layer and the apps may use.
const UI_ONLY: &[&str] = &["egui", "eframe", "egui_kittest", "winit", "rfd", "muda", "craft-quit"];
const UI_LAYER: u8 = 4;

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    let r = match task.as_str() {
        "ci" => ci(),
        "layers" => layers(),
        "bundle" => bundle(&flags()),
        "package-linux" => package_linux(&flags()),
        "package-windows" => package_windows(),
        "icon" => icon(),
        "web" => web(),
        _ => Err("usage: cargo xtask ci | layers | bundle [--universal] [--dmg] | package-linux [--target T] | package-windows | web | icon".into()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

fn flags() -> Vec<String> {
    std::env::args().skip(2).collect()
}

fn flag_value(flags: &[String], name: &str) -> Option<String> {
    flags.iter().position(|f| f == name).and_then(|i| flags.get(i + 1)).cloned()
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

fn cargo(args: &[&str]) -> Result<(), String> {
    eprintln!("» cargo {}", args.join(" "));
    let status = Command::new(env!("CARGO")).args(args).current_dir(root()).status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err(format!("cargo {} failed", args.join(" "))) }
}

fn ci() -> Result<(), String> {
    cargo(&["fmt", "--all", "--check"])?;
    cargo(&["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])?;
    cargo(&["test", "--workspace"])?;
    cargo(&["clippy", "-p", "pecunia-web", "--target", WASM, "--", "-D", "warnings"])?;
    layers()?;
    eprintln!("ci: all gates passed");
    Ok(())
}

fn layers() -> Result<(), String> {
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root())
        .output()
        .map_err(|e| e.to_string())?;
    let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let layer = |name: &str| LAYERS.iter().find(|(n, _)| *n == name).map(|(_, l)| *l);
    let mut problems = Vec::new();
    for pkg in meta["packages"].as_array().into_iter().flatten() {
        let name = pkg["name"].as_str().unwrap_or("");
        if name == "xtask" {
            continue;
        }
        let Some(mine) = layer(name) else {
            problems.push(format!("{name} has no layer in xtask LAYERS"));
            continue;
        };
        for dep in pkg["dependencies"].as_array().into_iter().flatten() {
            let dname = dep["name"].as_str().unwrap_or("");
            let dev = dep["kind"].as_str() == Some("dev");
            if let Some(theirs) = layer(dname)
                && theirs >= mine
            {
                problems.push(format!("{name} (L{mine}) depends on {dname} (L{theirs})"));
            }
            if !dev && mine < UI_LAYER && UI_ONLY.contains(&dname) {
                problems.push(format!("{name} (L{mine}) depends on UI crate {dname}"));
            }
        }
    }
    if problems.is_empty() {
        eprintln!("layers: ok");
        Ok(())
    } else {
        Err(format!("layering broken:\n  {}", problems.join("\n  ")))
    }
}

const WASM: &str = "wasm32-unknown-unknown";

/// Builds the web app: wasm, the JS glue from `wasm-bindgen`, and the static files.
fn web() -> Result<(), String> {
    let lock = std::fs::read_to_string(root().join("Cargo.lock")).map_err(|e| e.to_string())?;
    let want = lock
        .split("[[package]]")
        .find(|p| p.contains("name = \"wasm-bindgen\"\n"))
        .and_then(|p| p.lines().find_map(|l| l.strip_prefix("version = \"")))
        .map(|v| v.trim_end_matches('"').to_string())
        .ok_or("no wasm-bindgen in Cargo.lock")?;
    let have = Command::new("wasm-bindgen").arg("--version").output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    if !have.split_whitespace().any(|w| w == want) {
        return Err(format!("needs wasm-bindgen {want}: cargo install wasm-bindgen-cli --version {want} --locked"));
    }
    if Command::new("wasm-opt").arg("--version").output().is_err() {
        return Err("needs wasm-opt from binaryen (brew install binaryen, or a release from github.com/WebAssembly/binaryen)".into());
    }
    // Size over speed for the download: optimise for size, one codegen unit, full LTO (10.5 → 6 MB).
    eprintln!("» cargo build --release -p pecunia-web --target {WASM} (size settings)");
    let status = Command::new(env!("CARGO"))
        .args(["build", "--release", "-p", "pecunia-web", "--target", WASM])
        .env("CARGO_PROFILE_RELEASE_OPT_LEVEL", "s")
        .env("CARGO_PROFILE_RELEASE_LTO", "fat")
        .env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "1")
        .current_dir(root())
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("building the wasm failed".into());
    }
    let out = root().join("target/web");
    if out.exists() {
        std::fs::remove_dir_all(&out).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let wasm = root().join(format!("target/{WASM}/release/pecunia-web.wasm"));
    run("wasm-bindgen", &["--target", "web", "--no-typescript", "--out-dir", &out.display().to_string(), &wasm.display().to_string()])?;
    let bg = out.join("pecunia-web_bg.wasm").display().to_string();
    run("wasm-opt", &["-Oz", "--enable-bulk-memory", "--enable-nontrapping-float-to-int", "-o", &bg, &bg])?;
    let statics = root().join("apps/pecunia-web/static");
    for entry in std::fs::read_dir(&statics).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        std::fs::copy(entry.path(), out.join(entry.file_name())).map_err(|e| e.to_string())?;
    }
    std::fs::copy(root().join("packaging/linux/pecunia.png"), out.join("icon.png")).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(root().join("packaging/web")).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        std::fs::copy(entry.path(), out.join(entry.file_name())).map_err(|e| e.to_string())?;
    }
    service_worker(&out)?;
    eprintln!("web: built {}", out.display());
    Ok(())
}

/// Fills in `sw.js`: the files to keep for offline use, and a cache name that changes with them.
fn service_worker(out: &Path) -> Result<(), String> {
    fn walk(dir: &Path, out: &Path, files: &mut Vec<String>) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                walk(&path, out, files)?;
            } else if let Ok(rel) = path.strip_prefix(out) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                if rel != "sw.js" && rel != "_headers" && rel != "index.html" {
                    files.push(rel);
                }
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(out, out, &mut files)?;
    files.sort();
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for f in &files {
        std::hash::Hash::hash(f, &mut hash);
        std::hash::Hash::hash(&std::fs::read(out.join(f)).map_err(|e| e.to_string())?, &mut hash);
    }
    std::hash::Hash::hash(&std::fs::read(out.join("index.html")).map_err(|e| e.to_string())?, &mut hash);
    let cache = format!("pecunia-{}-{:016x}", env!("CARGO_PKG_VERSION"), std::hash::Hasher::finish(&hash));
    // The page itself is cached as "./" (the address people open).
    let list: Vec<String> = std::iter::once("./".to_string()).chain(files).map(|f| format!("{f:?}")).collect();
    let sw = out.join("sw.js");
    let text = std::fs::read_to_string(&sw).map_err(|e| e.to_string())?;
    if !text.contains("__CACHE__") || !text.contains("__FILES__") {
        return Err("sw.js has no __CACHE__ / __FILES__ to fill in".into());
    }
    let text = text.replace("\"__CACHE__\"", &format!("{cache:?}")).replace("__FILES__", &format!("[{}]", list.join(", ")));
    std::fs::write(&sw, text).map_err(|e| e.to_string())
}

fn run(cmd: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(cmd).args(args).current_dir(root()).status().map_err(|e| format!("{cmd}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{cmd} {} failed", args.join(" "))) }
}

fn mkdirs(p: &Path) -> Result<(), String> {
    std::fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to).map(|_| ()).map_err(|e| format!("copy {} → {}: {e}", from.display(), to.display()))
}

/// Renders the icon (drawn in code) and writes the committed `.icns` and Linux PNG. Needs a GPU
/// (wgpu offscreen), so it runs on a developer Mac, not in CI.
fn icon() -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("icon uses sips and iconutil; run it on a Mac".into());
    }
    let work = root().join("target/icon");
    let iconset = work.join("Pecunia.iconset");
    mkdirs(&iconset)?;
    let master = work.join("icon-1024.png");
    let master_s = master.display().to_string();
    cargo(&["run", "--release", "-p", "pecunia-ui-egui", "--example", "app_icon", "--", &master_s])?;
    for (size, name) in [
        (16, "icon_16x16.png"),
        (32, "icon_16x16@2x.png"),
        (32, "icon_32x32.png"),
        (64, "icon_32x32@2x.png"),
        (128, "icon_128x128.png"),
        (256, "icon_128x128@2x.png"),
        (256, "icon_256x256.png"),
        (512, "icon_256x256@2x.png"),
        (512, "icon_512x512.png"),
        (1024, "icon_512x512@2x.png"),
    ] {
        let out = iconset.join(name).display().to_string();
        let s = size.to_string();
        run("sips", &["-z", &s, &s, &master_s, "--out", &out])?;
    }
    let icns = root().join("packaging/macos/Pecunia.icns").display().to_string();
    run("iconutil", &["-c", "icns", &iconset.display().to_string(), "-o", &icns])?;
    copy(&iconset.join("icon_256x256@2x.png"), &root().join("packaging/linux/pecunia.png"))?;
    eprintln!("icon: updated packaging/macos/Pecunia.icns and packaging/linux/pecunia.png");
    Ok(())
}

const BINS: [&str; 2] = ["pecunia", "pecunia-cli"];

/// Builds both binaries for `target` (or the host) and returns the folder they land in.
fn build_release(target: Option<&str>) -> Result<PathBuf, String> {
    let mut args = vec!["build", "--release", "-p", "pecunia", "-p", "pecunia-cli"];
    if let Some(t) = target {
        args.extend(["--target", t]);
    }
    cargo(&args)?;
    Ok(match target {
        Some(t) => root().join("target").join(t).join("release"),
        None => root().join("target/release"),
    })
}

fn bundle(flags: &[String]) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("bundle builds a macOS .app; run it on a Mac".into());
    }
    let universal = flags.iter().any(|f| f == "--universal");
    let dmg = flags.iter().any(|f| f == "--dmg");
    let bins = root().join("target/release/bundle/bin");
    mkdirs(&bins)?;
    let arch = if universal {
        let arm = build_release(Some("aarch64-apple-darwin"))?;
        let intel = build_release(Some("x86_64-apple-darwin"))?;
        for b in BINS {
            let out = bins.join(b).display().to_string();
            run("lipo", &["-create", "-output", &out, &arm.join(b).display().to_string(), &intel.join(b).display().to_string()])?;
        }
        "universal"
    } else {
        let host = build_release(None)?;
        for b in BINS {
            copy(&host.join(b), &bins.join(b))?;
        }
        if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" }
    };

    let app = root().join("target/release/bundle/Pecunia.app");
    let contents = app.join("Contents");
    if app.exists() {
        std::fs::remove_dir_all(&app).map_err(|e| e.to_string())?;
    }
    mkdirs(&contents.join("MacOS"))?;
    mkdirs(&contents.join("Resources"))?;
    for b in BINS {
        copy(&bins.join(b), &contents.join("MacOS").join(b))?;
    }
    copy(&root().join("packaging/macos/Pecunia.icns"), &contents.join("Resources/Pecunia.icns"))?;
    std::fs::write(contents.join("Info.plist"), info_plist()).map_err(|e| e.to_string())?;
    // Ad-hoc signature: required on Apple Silicon; not a Developer ID (no notarisation).
    run("codesign", &["--force", "--deep", "--sign", "-", &app.display().to_string()])?;
    eprintln!("bundle: {}", app.display());

    if dmg {
        let dist = root().join("target/dist");
        mkdirs(&dist)?;
        // The disk image holds the app and a link to /Applications to drag it onto.
        let stage = root().join("target/release/bundle/dmg");
        if stage.exists() {
            std::fs::remove_dir_all(&stage).map_err(|e| e.to_string())?;
        }
        mkdirs(&stage)?;
        run("cp", &["-R", &app.display().to_string(), &stage.display().to_string()])?;
        run("ln", &["-s", "/Applications", &stage.join("Applications").display().to_string()])?;
        let out = dist.join(format!("Pecunia-{VERSION}-macos-{arch}.dmg"));
        run(
            "hdiutil",
            &["create", "-volname", "Pecunia", "-srcfolder", &stage.display().to_string(), "-ov", "-format", "UDZO", &out.display().to_string()],
        )?;
        // The CLI on its own, for people who only want it on their PATH.
        let cli = dist.join(format!("pecunia-cli-{VERSION}-macos-{arch}.tar.gz"));
        run("tar", &["-czf", &cli.display().to_string(), "-C", &bins.display().to_string(), "pecunia-cli"])?;
        eprintln!("dist: {}\ndist: {}", out.display(), cli.display());
    }
    Ok(())
}

fn package_linux(flags: &[String]) -> Result<(), String> {
    if !cfg!(target_os = "linux") {
        return Err("package-linux runs on Linux".into());
    }
    let target = flag_value(flags, "--target");
    let out_dir = build_release(target.as_deref())?;
    let arch = match target.as_deref().unwrap_or(std::env::consts::ARCH) {
        t if t.starts_with("aarch64") => "arm64",
        t if t.starts_with("x86_64") => "x86_64",
        other => other,
    };
    let name = format!("pecunia-{VERSION}-linux-{arch}");
    let dist = root().join("target/dist");
    let stage = dist.join(&name);
    if stage.exists() {
        std::fs::remove_dir_all(&stage).map_err(|e| e.to_string())?;
    }
    mkdirs(&stage)?;
    for b in BINS {
        copy(&out_dir.join(b), &stage.join(b))?;
    }
    copy(&root().join("packaging/linux/pecunia.desktop"), &stage.join("pecunia.desktop"))?;
    copy(&root().join("packaging/linux/pecunia.png"), &stage.join("pecunia.png"))?;
    copy(&root().join("README.md"), &stage.join("README.md"))?;
    let tarball = dist.join(format!("{name}.tar.gz"));
    run("tar", &["-czf", &tarball.display().to_string(), "-C", &dist.display().to_string(), &name])?;
    eprintln!("dist: {}", tarball.display());
    Ok(())
}

fn package_windows() -> Result<(), String> {
    if !cfg!(windows) {
        return Err("package-windows runs on Windows".into());
    }
    let out_dir = build_release(None)?;
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" };
    let name = format!("pecunia-{VERSION}-windows-{arch}");
    let dist = root().join("target/dist");
    let stage = dist.join(&name);
    if stage.exists() {
        std::fs::remove_dir_all(&stage).map_err(|e| e.to_string())?;
    }
    mkdirs(&stage)?;
    for b in BINS {
        let exe = format!("{b}.exe");
        copy(&out_dir.join(&exe), &stage.join(&exe))?;
    }
    copy(&root().join("README.md"), &stage.join("README.md"))?;
    // Windows 10+ ships bsdtar, which writes .zip with `-a`.
    let zip = dist.join(format!("{name}.zip"));
    run("tar", &["-a", "-c", "-f", &zip.display().to_string(), "-C", &dist.display().to_string(), &name])?;
    eprintln!("dist: {}", zip.display());
    Ok(())
}

fn info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Pecunia</string>
  <key>CFBundleDisplayName</key><string>Pecunia</string>
  <key>CFBundleIdentifier</key><string>nl.helico.pecunia</string>
  <key>CFBundleExecutable</key><string>pecunia</string>
  <key>CFBundleIconFile</key><string>Pecunia</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{VERSION}</string>
  <key>CFBundleVersion</key><string>{VERSION}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.finance</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>Pecunia book</string>
      <key>CFBundleTypeExtensions</key><array><string>pecunia</string></array>
      <key>CFBundleTypeRole</key><string>Editor</string>
      <key>LSHandlerRank</key><string>Owner</string>
    </dict>
  </array>
</dict>
</plist>
"#
    )
}
