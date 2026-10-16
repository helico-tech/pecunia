#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

//! Pecunia in the browser (pecunia.helico.app). The same egui UI as the desktop app, compiled to
//! wasm. Local-first: the book is a `.pecunia` file on the person's own disk, opened and saved
//! through the browser's File System Access API; bank files are picked with the browser's file
//! picker and passed to the engine as bytes (ADR 0005). Build with `cargo xtask web`.

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod file_store;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("pecunia-web runs in a browser: build it with `cargo xtask web` and serve target/web/.");
}
