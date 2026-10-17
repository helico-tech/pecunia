// File helpers for the Pecunia web app, called from Rust (src/web.rs) through wasm-bindgen: the
// File System Access API for the book, and a file picker for bank files. Every function that
// touches a file is async and may reject; Rust shows the error.
//
// Browsers without that API (phones, Safari, Firefox) keep books in the browser's own storage
// (IndexedDB, ADR 0009). Such a book has a "browser handle", `{ browser: true, name }`, which the
// same functions read and write, so Rust treats both kinds alike.

const TYPES = [{ description: "Pecunia book", accept: { "application/json": [".pecunia"] } }];

// Bank files Pecunia reads: CSV of Dutch banks, CAMT.053 (XML) and MT940. The pickers still offer
// "All files", since banks name their exports in many ways.
const BANK_EXTENSIONS = [".csv", ".txt", ".xml", ".sta", ".940", ".tab"];
const BANK_TYPES = [{ description: "Bank file (CSV, CAMT.053, MT940)", accept: { "text/plain": BANK_EXTENSIONS } }];

// The engine refuses bank files and books larger than these; don't read them into memory at all.
const MAX_BANK_FILE = 64 * 1024 * 1024;
const MAX_BOOK = 512 * 1024 * 1024;

export function supported() {
  return typeof window.showOpenFilePicker === "function" && typeof window.showSaveFilePicker === "function";
}

function cancelled(e) {
  return e && e.name === "AbortError";
}

function inBrowser(handle) {
  return handle && handle.browser === true;
}

/**
 * The picked file's handle, or null when the person cancels. Without the File System Access API
 * the picked file is copied into the browser's storage (under a free name) and that copy opens.
 */
export async function pickOpen() {
  if (!supported()) {
    // No `accept`: iOS greys out files with extensions it doesn't know, such as .pecunia.
    const file = await pickWithInput("");
    if (!file) return null;
    if (file.size > MAX_BOOK) throw new Error(`${file.name} is larger than 512 MB`);
    const bytes = new Uint8Array(await file.arrayBuffer());
    const name = await freeName(file.name);
    await putBook(name, bytes);
    return { browser: true, name };
  }
  try {
    const [handle] = await window.showOpenFilePicker({ id: "pecunia", types: TYPES });
    return handle ?? null;
  } catch (e) {
    if (cancelled(e)) return null;
    throw e;
  }
}

/** A handle for a new file, or null when the person cancels. In the browser's storage: that name. */
export async function pickSave(suggestedName) {
  if (!supported()) return { browser: true, name: suggestedName };
  try {
    return await window.showSaveFilePicker({ id: "pecunia", types: TYPES, suggestedName });
  } catch (e) {
    if (cancelled(e)) return null;
    throw e;
  }
}

/** Whether the page may write the file; with `ask`, the browser asks the person. */
export async function allowWrite(handle, ask) {
  if (inBrowser(handle)) {
    // Ask the browser not to clear its storage when space runs low; it may say no.
    await navigator.storage?.persist?.().catch(() => false);
    return true;
  }
  const opts = { mode: "readwrite" };
  if ((await handle.queryPermission(opts)) === "granted") return true;
  if (!ask) return false;
  return (await handle.requestPermission(opts)) === "granted";
}

export async function readFile(handle) {
  if (inBrowser(handle)) {
    const rec = await getBook(handle.name);
    if (!rec) throw new Error("it is no longer in this browser");
    return { name: handle.name, bytes: rec.bytes, modified: rec.modified, size: rec.bytes.length };
  }
  const file = await handle.getFile();
  return { name: handle.name, bytes: new Uint8Array(await file.arrayBuffer()), modified: file.lastModified, size: file.size };
}

export async function statFile(handle) {
  if (inBrowser(handle)) {
    const rec = await getBook(handle.name);
    return rec ? { modified: rec.modified, size: rec.bytes.length } : { modified: 0, size: 0 };
  }
  const file = await handle.getFile();
  return { modified: file.lastModified, size: file.size };
}

/** Replaces the file's contents. The browser writes a temporary file and swaps it in on close. */
export async function writeFile(handle, bytes) {
  // `bytes` is a view into wasm memory, which may move once we await: copy it first.
  const copy = bytes.slice();
  if (inBrowser(handle)) {
    const rec = await putBook(handle.name, copy);
    return { modified: rec.modified, size: copy.length };
  }
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
    file = await pickWithInput(BANK_EXTENSIONS.join(","));
  }
  if (!file) return null;
  if (file.size > MAX_BANK_FILE) throw new Error(`${file.name} is larger than 64 MB`);
  return { name: file.name, bytes: new Uint8Array(await file.arrayBuffer()) };
}

function pickWithInput(accept) {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    if (accept) input.accept = accept;
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

// IndexedDB keeps the last file's handle, so the app can offer to reopen it after a reload, and
// ("books") the books kept in the browser: `{ bytes, modified }` by file name.
function db() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open("pecunia", 2);
    req.onupgradeneeded = () => {
      for (const name of ["handles", "books"]) {
        if (!req.result.objectStoreNames.contains(name)) req.result.createObjectStore(name);
      }
    };
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

async function getBook(name) {
  const d = await db();
  try {
    return await new Promise((resolve, reject) => {
      const req = d.transaction("books").objectStore("books").get(name);
      req.onsuccess = () => resolve(req.result ?? null);
      req.onerror = () => reject(req.error);
    });
  } finally {
    d.close();
  }
}

/** Stores a book; `modified` always moves forward, so other tabs notice the change. */
async function putBook(name, bytes) {
  const d = await db();
  try {
    return await new Promise((resolve, reject) => {
      const tx = d.transaction("books", "readwrite");
      const books = tx.objectStore("books");
      let rec = null;
      const old = books.get(name);
      old.onsuccess = () => {
        rec = { bytes, modified: Math.max(Date.now(), (old.result?.modified ?? 0) + 1) };
        books.put(rec, name);
      };
      tx.oncomplete = () => resolve(rec);
      tx.onerror = () => reject(tx.error);
      tx.onabort = () => reject(tx.error ?? new Error("the browser's storage refused it (full?)"));
    });
  } finally {
    d.close();
  }
}

/** The names of the books kept in this browser, sorted. */
export async function listBooks() {
  const d = await db();
  try {
    const names = await new Promise((resolve, reject) => {
      const req = d.transaction("books").objectStore("books").getAllKeys();
      req.onsuccess = () => resolve(req.result.map(String));
      req.onerror = () => reject(req.error);
    });
    return names.sort((a, b) => a.localeCompare(b));
  } finally {
    d.close();
  }
}

/** A browser handle for a book kept in this browser. */
export function browserBook(name) {
  return { browser: true, name };
}

/** `name`, or `name (2)`, `name (3)`, … when the browser already keeps a book by that name. */
async function freeName(name) {
  const taken = new Set(await listBooks());
  if (!taken.has(name)) return name;
  const dot = name.lastIndexOf(".");
  const [stem, ext] = dot > 0 ? [name.slice(0, dot), name.slice(dot)] : [name, ""];
  for (let i = 2; ; i++) {
    const candidate = `${stem} (${i})${ext}`;
    if (!taken.has(candidate)) return candidate;
  }
}

/** Offers `bytes` as a download named `name` (a copy of the book). */
export function download(name, bytes) {
  const url = URL.createObjectURL(new Blob([bytes.slice()], { type: "application/octet-stream" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.style.display = "none";
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
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
