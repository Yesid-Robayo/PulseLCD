// Armazón de la interfaz: estado, navegación y utilidades. Cada página vive en pages.js y el editor en editor.js.
import { renderPage } from "./pages.js";
import { setLanguage, t } from "./strings.js";

const POLL_MS = 1000;
let updateChecks = 0;

export const app = { state: null, page: "home", update: null, locked: false, newVersion: null };

const ICONS = {
  home: '<rect x="3" y="5" width="18" height="12" rx="2"/><path d="M8 21h8M12 17v4"/>',
  themes: '<rect x="3" y="3" width="8" height="8" rx="2"/><rect x="13" y="3" width="8" height="8" rx="2"/><rect x="3" y="13" width="8" height="8" rx="2"/><path d="M14 17h6M17 14v6"/>',
  sensors: '<path d="M3 12h4l2-6 4 12 3-9 2 3h3"/>',
  alerts: '<path d="M6 16V11a6 6 0 1 1 12 0v5l2 2H4z"/><path d="M10 21h4"/>',
  weather: '<path d="M7 18a4 4 0 0 1-.6-8A6 6 0 0 1 18 11a3.5 3.5 0 0 1-.5 7z"/>',
  games: '<rect x="2" y="7" width="20" height="11" rx="5"/><path d="M7 11v3M5.5 12.5h3M16 12h.01M18 14h.01"/>',
  settings: '<circle cx="12" cy="12" r="3"/><path d="M19 12a7 7 0 0 0-.1-1.2l2-1.5-2-3.4-2.3.9a7 7 0 0 0-2-1.2L14 3h-4l-.5 2.6a7 7 0 0 0-2 1.2l-2.4-1-2 3.5 2 1.5A7 7 0 0 0 5 12a7 7 0 0 0 .1 1.2l-2 1.5 2 3.4 2.4-.9a7 7 0 0 0 2 1.2L10 21h4l.5-2.6a7 7 0 0 0 2-1.2l2.3.9 2-3.4-2-1.5c.1-.4.2-.8.2-1.4z"/>',
  pulse: '<path d="M3 13h5l2-7 4 13 3-9 1.5 3H22"/>',
  repair: '<path d="M21 7a5 5 0 0 1-7 6l-7 7-3-3 7-7a5 5 0 0 1 6-7l-3 3 1 3 3 1z"/>',
  warn: '<path d="M12 3 2 20h20z"/><path d="M12 10v5M12 18h.01"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  close: '<path d="m6 6 12 12M18 6 6 18"/>',
  up: '<path d="m6 15 6-6 6 6"/>',
  down: '<path d="m6 9 6 6 6-6"/>',
  undo: '<path d="M9 7 4 12l5 5"/><path d="M4 12h11a5 5 0 0 1 0 10h-3"/>',
  redo: '<path d="m15 7 5 5-5 5"/><path d="M20 12H9a5 5 0 0 0 0 10h3"/>',
  text: '<path d="M5 6h14M12 6v13"/>',
  value: '<path d="M6 6h12M6 12h12M6 18h7"/>',
  bar: '<rect x="3" y="9" width="18" height="6" rx="3"/><path d="M6 12h8"/>',
  ring: '<circle cx="12" cy="12" r="8"/><path d="M12 4a8 8 0 0 1 8 8"/>',
  graph: '<path d="M3 17l5-6 4 3 5-8 4 5"/>',
  panel: '<rect x="4" y="5" width="16" height="14" rx="3"/>',
  trash: '<path d="M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13"/>',
  copy: '<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M5 16H4V4h12v1"/>',
  edit: '<path d="M4 20l1-5L16 4l4 4L9 19z"/>',
  back: '<path d="m15 6-6 6 6 6"/>',
  check: '<path d="m5 12 5 5 9-10"/>',
  update: '<path d="M12 20V6M6 12l6-6 6 6"/>',
  heart: '<path d="M12 20S4 15 4 9.5A4.5 4.5 0 0 1 12 7a4.5 4.5 0 0 1 8 2.500C20 15 12 20 12 20z"/>',
};

export function icon(name, className = "") {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("class", `icon ${className}`);
  svg.innerHTML = ICONS[name] ?? "";
  return svg;
}

// crea un elemento: h("div.card", { onclick }, hijo1, hijo2)
export function h(spec, props, ...children) {
  const [tag, ...classes] = spec.split(".");
  const el = document.createElement(tag || "div");
  if (classes.length) el.className = classes.join(" ");
  if (props instanceof Node || typeof props === "string" || Array.isArray(props)) {
    children.unshift(props);
    props = null;
  }
  for (const [key, value] of Object.entries(props ?? {})) {
    if (value === undefined || value === null || value === false) continue;
    if (key.startsWith("on")) el.addEventListener(key.slice(2), value);
    else if (key === "value") el.value = value;
    else if (key === "checked") el.checked = value;
    else if (key === "class") el.className += ` ${value}`;
    else el.setAttribute(key, value === true ? "" : value);
  }
  for (const child of children.flat(Infinity)) {
    if (child !== null && child !== undefined && child !== false) el.append(child);
  }
  return el;
}

export async function api(path, options) {
  const response = await fetch(`/api/${path}`, options);
  const body = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(body.error ?? "error");
  return body;
}

export const post = (path, body) =>
  api(path, { method: "POST", body: body === undefined ? undefined : JSON.stringify(body) });

export const action = (name) => post(`action?name=${encodeURIComponent(name)}`);

export function image(path) {
  return `/api/${path}`;
}

let toastTimer;
export function toast(text) {
  const el = document.getElementById("toast");
  el.textContent = text;
  el.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.remove("show"), 2600);
}

export function closeDialog() {
  const overlay = document.getElementById("overlay");
  overlay.classList.remove("open");
  overlay.replaceChildren();
}

export function dialog(content) {
  const overlay = document.getElementById("overlay");
  overlay.replaceChildren(content);
  overlay.classList.add("open");
}

// pregunta y resuelve a verdadero o falso
export function confirmBox(title, note, yes, danger = false) {
  return new Promise((resolve) => {
    const answer = (value) => {
      closeDialog();
      resolve(value);
    };
    dialog(h("div.dialog", h("h2", title), note && h("p", note),
      h("div.actions",
        h("button.btn", { onclick: () => answer(false) }, t("cancel")),
        h(`button.btn.${danger ? "danger" : "primary"}`, { onclick: () => answer(true) }, yes))));
  });
}

export function askText(title, initial, yes) {
  return new Promise((resolve) => {
    const field = h("input", { type: "text", value: initial, maxlength: 40 });
    const answer = (value) => {
      closeDialog();
      resolve(value);
    };
    field.addEventListener("keydown", (e) => {
      if (e.key === "Enter") answer(field.value.trim() || null);
      if (e.key === "Escape") answer(null);
    });
    dialog(h("div.dialog", h("h2", title), field,
      h("div.actions",
        h("button.btn", { onclick: () => answer(null) }, t("cancel")),
        h("button.btn.primary", { onclick: () => answer(field.value.trim() || null) }, yes))));
    field.select();
  });
}

// cuenta los cambios: una consulta que salió antes de un cambio traería la configuración vieja
let configChanges = 0;

// cambia la configuración: aplica el cambio aquí y se lo pasa al agente, el único que escribe el archivo
export async function setConfig(change) {
  const config = structuredClone(app.state.config);
  change(config);
  configChanges += 1;
  app.state.config = config;
  applyLook(config);
  await post("config", config).catch((e) => toast(e.message));
}

function applyLook(config) {
  setLanguage(config.language);
  const dark = matchMedia("(prefers-color-scheme: dark)").matches;
  const light = config.appearance === "light" || (config.appearance === "system" && !dark);
  document.documentElement.dataset.theme = light ? "light" : "dark";
}

export function stateChip() {
  const state = app.state;
  const panel = state.status?.state ?? "searching";
  if (!state.connected) return { dot: "warn", text: t("noAgent") };
  const looks = {
    healthy: ["good", "stateHealthy"],
    searching: ["bad", "stateSearching"],
    waking: ["warn", "stateWaking"],
    syncing: ["warn", "stateSyncing"],
    off: ["", "stateOff"],
    maintenance: ["warn", "stateMaintenance"],
  };
  const [dot, key] = looks[panel] ?? looks.searching;
  return { dot, text: t(key) };
}

const NAV = ["home", "themes", "sensors", "alerts", "weather", "games", "settings"];

const connection = h("div.connection");

// solo cambia esto cada segundo: rehacer los botones perdería el clic que coincida con ese instante
function paintConnection() {
  const chip = stateChip();
  const port = app.state.status?.port;
  if (connection.dataset.shown === `${chip.dot}|${chip.text}|${port}`) return;
  connection.dataset.shown = `${chip.dot}|${chip.text}|${port}`;
  connection.replaceChildren(h(`span.dot.${chip.dot}`), h("div", chip.text, port && h("small", port)));
}

function drawRail() {
  document.getElementById("rail").replaceChildren(
    h("div.brand", h("div.brand-mark", icon("pulse")), h("strong", "PulseLCD")),
    ...NAV.map((page) => h(`button.nav${app.page === page ? ".on" : ""}`, { onclick: () => go(page) }, icon(page), t(page))),
    h("div.rail-space"),
    connection,
  );
  paintConnection();
}

export function go(page) {
  if (app.locked) return;
  app.page = page;
  draw();
}

export function draw() {
  applyLook(app.state.config);
  drawRail();
  const target = document.getElementById("page");
  const { element, update } = renderPage(app.page);
  target.replaceChildren(element);
  target.scrollTop = 0;
  app.update = update ?? null;
}

// solo se ve aquí, así que solo se pregunta con la ventana abierta. Con el aviso apagado el servidor no sale a la red
export async function checkForUpdate() {
  const check = ++updateChecks;
  const version = (await api("update").catch(() => ({}))).version ?? null;
  // una respuesta lenta no pisa a la de después: quizá el aviso ya se apagó
  if (check === updateChecks) app.newVersion = version;
}

async function poll() {
  try {
    const before = app.state;
    const changes = configChanges;
    const state = await api("state");
    if (before && changes !== configChanges) state.config = app.state.config;
    app.state = state;
    if (!before) {
      if (window.__page) app.page = window.__page;
      draw();
      checkForUpdate();
    }
    else {
      paintConnection();
      app.update?.(app.state);
    }
  } catch {
    // la vista web puede ir por delante del proceso al arrancar: se reintenta en el siguiente ciclo
  }
  setTimeout(poll, POLL_MS);
}

document.addEventListener("contextmenu", (e) => {
  if (!e.target.closest("input")) e.preventDefault();
});

poll();
