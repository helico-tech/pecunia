// File helpers for the Pecunia web app, called from Rust (src/web.rs) through wasm-bindgen: the
// File System Access API for the book, and a file picker for bank files. Every function that
// touches a file is async and may reject; Rust shows the error.

const TYPES = [{ description: "Pecunia book", accept: { "application/json": [".pecunia"] } }];

// Bank files Pecunia reads: CSV of Dutch banks, CAMT.053 (XML) and MT940. The pickers still offer
// "All files", since banks name their exports in many ways.
const BANK_EXTENSIONS = [".csv", ".txt", ".xml", ".sta", ".940", ".tab"];
const BANK_TYPES = [{ description: "Bank file (CSV, CAMT.053, MT940)", accept: { "text/plain": BANK_EXTENSIONS } }];

// The engine refuses bank files larger than this; don't read them into memory at all.
const MAX_BANK_FILE = 64 * 1024 * 1024;

export function supported() {
  return typeof window.showOpenFilePicker === "function" && typeof window.showSaveFilePicker === "function";
}

function cancelled(e) {
  return e && e.name === "AbortError";
}

/** The picked file's handle, or null when the person cancels. */
export async function pickOpen() {
  try {
    const [handle] = await window.showOpenFilePicker({ id: "pecunia", types: TYPES });
    return handle ?? null;
  } catch (e) {
    if (cancelled(e)) return null;
    throw e;
  }
}

/** A handle for a new file, or null when the person cancels. */
export async function pickSave(suggestedName) {
  try {
    return await window.showSaveFilePicker({ id: "pecunia", types: TYPES, suggestedName });
  } catch (e) {
    if (cancelled(e)) return null;
    throw e;
  }
}

/** Whether the page may write the file; with `ask`, the browser asks the person. */
export async function allowWrite(handle, ask) {
  const opts = { mode: "readwrite" };
  if ((await handle.queryPermission(opts)) === "granted") return true;
  if (!ask) return false;
  return (await handle.requestPermission(opts)) === "granted";
}

export async function readFile(handle) {
  const file = await handle.getFile();
  return { name: handle.name, bytes: new Uint8Array(await file.arrayBuffer()), modified: file.lastModified, size: file.size };
}

export async function statFile(handle) {
  const file = await handle.getFile();
  return { modified: file.lastModified, size: file.size };
}

/** Replaces the file's contents. The browser writes a temporary file and swaps it in on close. */
export async function writeFile(handle, bytes) {
  // `bytes` is a view into wasm memory, which may move once we await: copy it first.
  const copy = bytes.slice();
  const out = await handle.createWritable();
  try {
    await out.write(copy);
    await out.close();
  } catch (e) {
    await out.abort().catch(() => {});
    throw e;
  }
  return statFile(handle);
}

/**
 * Asks for a bank file to import: `{ name, bytes }`, or null when the person cancels. Uses
 * `showOpenFilePicker` where the browser has it, else a hidden `<input type=file>`, which works in
 * every browser. Only reads the file; nothing is kept.
 */
export async function pickBankFile() {
  let file = null;
  if (typeof window.showOpenFilePicker === "function") {
    try {
      const [handle] = await window.showOpenFilePicker({ id: "pecunia-import", types: BANK_TYPES });
      file = handle ? await handle.getFile() : null;
    } catch (e) {
      if (cancelled(e)) return null;
      throw e;
    }
  } else {
    file = await pickWithInput();
  }
  if (!file) return null;
  if (file.size > MAX_BANK_FILE) throw new Error(`${file.name} is larger than 64 MB`);
  return { name: file.name, bytes: new Uint8Array(await file.arrayBuffer()) };
}

function pickWithInput() {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = BANK_EXTENSIONS.join(",");
    input.style.display = "none";
    const done = (file) => {
      input.remove();
      resolve(file);
    };
    input.addEventListener("change", () => done(input.files?.[0] ?? null), { once: true });
    // Browsers without the `cancel` event leave the promise pending, which does no harm.
    input.addEventListener("cancel", () => done(null), { once: true });
    document.body.appendChild(input);
    input.click();
  });
}

// IndexedDB keeps the last file's handle, so the app can offer to reopen it after a reload.
function db() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open("pecunia", 1);
    req.onupgradeneeded = () => req.result.createObjectStore("handles");
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

export async function remember(handle) {
  const d = await db();
  try {
    await new Promise((resolve, reject) => {
      const tx = d.transaction("handles", "readwrite");
      tx.objectStore("handles").put(handle, "last");
      tx.oncomplete = resolve;
      tx.onerror = () => reject(tx.error);
    });
  } finally {
    d.close();
  }
}

export async function lastHandle() {
  const d = await db();
  try {
    return await new Promise((resolve, reject) => {
      const req = d.transaction("handles").objectStore("handles").get("last");
      req.onsuccess = () => resolve(req.result ?? null);
      req.onerror = () => reject(req.error);
    });
  } finally {
    d.close();
  }
}

export function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function setTitle(title) {
  document.title = title;
}

export function ready() {
  document.getElementById("loading")?.remove();
}

export function failed(message) {
  const el = document.getElementById("loading");
  if (el) el.textContent = message;
}

// The chosen appearance (system, light, dark), kept per browser. Storage can be blocked: then the
// app just starts with "system".
export function loadTheme() {
  try {
    return localStorage.getItem("pecunia.theme") ?? "";
  } catch {
    return "";
  }
}

export function saveTheme(id) {
  try {
    localStorage.setItem("pecunia.theme", id);
  } catch {}
}

/** `n` bytes from the browser's cryptographic random source (salts and nonces for protected books). */
export function randomBytes(n) {
  const a = new Uint8Array(n);
  crypto.getRandomValues(a);
  return a;
}
