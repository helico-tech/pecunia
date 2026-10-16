// Loads the wasm app (built by `cargo xtask web`). Its `main` starts eframe on <canvas id="app">.
import init from "./pecunia-web.js";

init().catch((e) => {
  const el = document.getElementById("loading");
  if (el) el.textContent = `Pecunia couldn't start: ${e}`;
});
