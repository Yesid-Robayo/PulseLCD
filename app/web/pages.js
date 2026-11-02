// Las páginas de la ventana. Cada una devuelve su elemento y, si lo necesita, una función que refresca solo lo que cambia.
import { action, api, app, askText, checkForUpdate, closeDialog, confirmBox, dialog, draw, go, h, icon, image, post, setConfig, stateChip, toast } from "./app.js";
import { openEditor } from "./editor.js";
import { metricName, t } from "./strings.js";

const value = (status, metric) => status?.readings?.values?.[metric];

// mismo formato que el panel, para lo que la ventana muestra por su cuenta
function show(metric, reading) {
  if (!reading || reading === "missing") return "--";
  if (reading.text !== undefined) return reading.text;
  const n = reading.number;
  if (n === undefined || n === null) return "--";
  const round = Math.round(n);
  switch (metric) {
    case "cpu_temp": case "gpu_temp": return `${round}°C`;
    case "weather_temp": case "weather_feels": return `${round}°`;
    case "gpu_memory_used": return `${round} MB`;
    case "gpu_clock": return `${n.toFixed(2)} GHz`;
    case "fps": return `${round}`;
    case "ram_used": case "disk_used": case "disk_free": return `${round} GB`;
    case "net_down": case "net_up": return `${bytes(n)}/s`;
    case "net_down_total": case "net_up_total": return bytes(n);
    case "ping": return `${round} ms`;
    case "uptime": return duration(n);
    default: return `${round} %`;
  }
}

function bytes(n) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1024 && i < units.length - 1) { n /= 1024; i += 1; }
  return `${n.toFixed(i === 0 || n >= 100 ? 0 : 1)} ${units[i]}`;
}

function duration(seconds) {
  const d = Math.floor(seconds / 86400);
  const hh = String(Math.floor((seconds % 86400) / 3600)).padStart(2, "0");
  const mm = String(Math.floor((seconds % 3600) / 60)).padStart(2, "0");
  return d ? `${d}d ${hh}:${mm}` : `${hh}:${mm}`;
}

function toggle(checked, onchange) {
  const input = h("input", { type: "checkbox", checked, onchange: () => onchange(input.checked) });
  return h("label.switch", input, h("i"));
}

function slider(min, max, val, oninput, onchange) {
  const fill = (el) => el.style.setProperty("--fill", `${((el.value - min) / (max - min)) * 100}%`);
  const input = h("input", { type: "range", min, max, value: val, oninput: () => { fill(input); oninput?.(+input.value); }, onchange: () => onchange?.(+input.value) });
  fill(input);
  return input;
}

function segments(options, current, onpick) {
  const box = h("div.segments");
  for (const [id, label] of options) {
    box.append(h(`button${id === current ? ".on" : ""}`, {
      onclick: () => {
        [...box.children].forEach((b) => b.classList.remove("on"));
        box.children[options.findIndex((o) => o[0] === id)].classList.add("on");
        onpick(id);
      },
    }, label));
  }
  return box;
}

function setting(title, note, control) {
  return h("div.setting", h("div", h("b", title), note && h("span.note", note)), control);
}

function head(title, sub, ...actions) {
  return h("div.head", h("div", h("h1", title), sub && h("p", sub)), actions.length ? h("div.row", ...actions) : null);
}

// con qué se miden los FPS en este PC, dicho para una persona
function fpsToolName() {
  const names = { amd_driver: "fpsToolAmd", nvidia_driver: "fpsToolNvidia", included: "fpsToolIncluded" };
  return t(names[app.state.fps_tool] ?? "fpsToolNone");
}

function capitalized(text) {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

// explica qué es el ayudante y qué cambia en el PC antes de que Windows pida permiso
export function helperDialog(removing) {
  const installed = removing;
  const copies = app.state.fps_tool === "included" ? "helperInstall1Included" : "helperInstall1";
  const cpu = app.state.pawnio ? "helperCpu" : "helperCpuMissing";
  const run = async () => {
    closeDialog();
    const result = await action(installed ? "remove_helper" : "install_helper").catch(() => ({ ok: false }));
    const failure = { denied: "helperDenied", other_account: "helperOtherAccount" }[result.error] ?? "helperFailed";
    toast(t(result.ok ? (installed ? "helperRemoved" : "helperInstalled") : failure));
  };
  dialog(h("div.dialog",
    h("h2", t("helperWhat")),
    h("p", t("helperIntro")),
    h("h3", t(installed ? "helperRemoveTitle" : "helperInstallTitle")),
    h("ul", (installed ? ["helperRemove1", "helperRemove2", "helperRemove3"] : [copies, "helperInstall2", "helperGame", cpu, "helperInstall3", "helperInstall4"]).map((key) => h("li", t(key, { tool: fpsToolName() })))),
    h("div.actions",
      h("button.btn", { onclick: closeDialog }, t("cancel")),
      installed
        ? h("button.btn.danger", { onclick: run }, t("helperRemove"))
        : h("button.btn.primary", { onclick: run }, t("helperInstall")))));
}

// al arrancar el PC tarda unos segundos en responder: solo se avisa si sigue callado
const HELPER_GRACE_MS = 30000;
let helperSilentSince = null;

function helperIsDown(s) {
  const silent = s.helper && !s.helper_outdated && s.status?.cpu_temp === "needs_helper";
  if (!silent) helperSilentSince = null;
  else helperSilentSince ??= Date.now();
  return silent && Date.now() - helperSilentSince > HELPER_GRACE_MS;
}

function notices() {
  const s = app.state;
  const list = [];
  if (app.newVersion) {
    list.push(h("div.notice.info", icon("update"), h("div.grow", h("b", t("updateTitle", { version: app.newVersion })), h("span", t("updateNote", { version: s.version }))),
      h("button.btn", { onclick: () => action("releases") }, t("updateGet"))));
  }
  if (s.theme_needs_weather) {
    list.push(h("div.notice.info", icon("weather"), h("div.grow", h("b", t("themeWeatherTitle")), h("span", t("themeWeatherNote"))),
      h("button.btn", { onclick: () => go("weather") }, t("themeWeatherGet"))));
  }
  if (s.helper_outdated) {
    list.push(h("div.notice", icon("warn"), h("div.grow", h("b", t("helperUpdateTitle")), h("span", t("helperOutdatedNote"))),
      h("button.btn.primary", { onclick: () => helperDialog(false) }, t("helperReinstall"))));
  }
  if (helperIsDown(s)) {
    list.push(h("div.notice", icon("warn"), h("div.grow", h("b", t("helperDownTitle")), h("span", t("helperDownNote"))),
      h("button.btn.primary", { onclick: () => helperDialog(false) }, t("helperReinstall"))));
  }
  if (s.status?.cpu_temp === "needs_helper" && !s.helper) {
    list.push(h("div.notice", icon("warn"), h("div.grow", h("b", t("helperTitle")), h("span", t("helperNote"))),
      h("button.btn.primary", { onclick: () => helperDialog(false) }, t("helperEnable"))));
  } else if (s.status?.cpu_temp === "needs_driver") {
    list.push(h("div.notice", icon("warn"), h("div.grow", h("b", t("pawnioTitle")), h("span", t("pawnioNote"))),
      h("button.btn", { onclick: () => action("pawnio") }, t("pawnioGet"))));
  }
  return list;
}

// corre cada segundo: si nada cambió no se toca, porque un clic que coincide con el cambio de botón se pierde
function showNotices(target, list) {
  const text = list.map((notice) => notice.textContent).join("\n");
  if (target.dataset.shown === text) return;
  target.dataset.shown = text;
  target.replaceChildren(...list);
}

// ---- inicio ----
const MIN_TRAIL = 4;

function sparkline() {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 100 40");
  svg.setAttribute("preserveAspectRatio", "none");
  svg.setAttribute("class", "spark");
  const line = document.createElementNS("http://www.w3.org/2000/svg", "path");
  line.setAttribute("fill", "color-mix(in srgb, var(--accent) 22%, transparent)");
  line.setAttribute("stroke", "var(--accent)");
  line.setAttribute("stroke-width", "1");
  line.setAttribute("vector-effect", "non-scaling-stroke");
  svg.append(line);
  svg.draw = (points) => {
    if (points.length < MIN_TRAIL) return;
    const top = Math.max(1, ...points);
    const step = 100 / (points.length - 1);
    const path = points.map((p, i) => `${i ? "L" : "M"}${(i * step).toFixed(1)} ${(38 - (p / top) * 30).toFixed(1)}`).join("");
    line.setAttribute("d", `${path}L100 40L0 40Z`);
  };
  return svg;
}

const trail = {};
const TRAIL_LENGTH = 60;

function pushTrail(metric, reading) {
  const list = (trail[metric] ??= []);
  list.push(reading?.number ?? 0);
  if (list.length > TRAIL_LENGTH) list.shift();
  return list;
}

function homePage() {
  const s = app.state;
  const screen = h("img", { alt: "" });
  const bezel = h("div.bezel", screen);
  const stateBadge = h("span.chip");
  const stage = h("div.stage", h("div.stage-state", stateBadge), bezel);
  const tiles = [["cpu_usage", "CPU", "cpu_temp"], ["gpu_usage", "GPU", "gpu_temp"], ["ram_percent", "RAM", "ram_used"], ["fps", "FPS", null]]
    .map(([metric, label, extra]) => {
      const number = h("strong", "--");
      const detail = h("em", "");
      const spark = sparkline();
      return { metric, extra, number, detail, spark, element: h("div.tile", h("small", label), number, detail, spark) };
    });
  const frame = h("b", "--");
  const reconnects = h("b", "0");
  const healed = h("b", "0");
  const port = h("b", "--");
  const warnings = h("div.stack");
  let shownFrame = -1;
  let dragging = false;

  const brightness = slider(0, 100, s.config.panel.brightness,
    (v) => { dragging = true; brightnessText.textContent = `${v} %`; setBrightness(v); },
    () => { dragging = false; });
  const brightnessText = h("span.muted", `${s.config.panel.brightness} %`);
  let brightnessTimer;
  function setBrightness(v) {
    clearTimeout(brightnessTimer);
    brightnessTimer = setTimeout(() => setConfig((c) => { c.panel.brightness = v; }), 80);
  }
  const screenSwitch = toggle(s.status?.state !== "off", (on) => action(on ? "screen_on" : "screen_off"));

  const themeName = h("b.grow", s.config.panel.theme);
  api("themes").then((list) => {
    themeName.textContent = list.find((x) => x.id === s.config.panel.theme)?.name ?? s.config.panel.theme;
  });

  const update = (state) => {
    const status = state.status;
    const chip = stateChip();
    if (stateBadge.dataset.shown !== `${chip.dot}|${chip.text}`) {
      stateBadge.dataset.shown = `${chip.dot}|${chip.text}`;
      stateBadge.className = `chip ${chip.dot}`;
      stateBadge.replaceChildren(h(`span.dot.${chip.dot}`), chip.text);
    }
    if (state.frame !== shownFrame) {
      shownFrame = state.frame;
      if (state.frame > 0) screen.src = image(`frame?s=${state.frame}`);
    }
    for (const tile of tiles) {
      const reading = value(status, tile.metric);
      tile.number.textContent = show(tile.metric, reading);
      tile.detail.textContent = tile.extra ? show(tile.extra, value(status, tile.extra)) : status?.foreground ?? "";
      tile.spark.draw(pushTrail(tile.metric, reading));
    }
    frame.textContent = status?.frame_ms ? `${(status.frame_ms / 1000).toFixed(2)} s` : "--";
    reconnects.textContent = status?.reconnects ?? 0;
    healed.textContent = status?.healed ?? 0;
    port.textContent = status?.port ?? "--";
    const list = notices();
    if ((status?.missing_secs ?? 0) >= 30) {
      list.unshift(h("div.notice.bad", icon("warn"), h("div.grow", h("b", t("unplugged")), h("span", t("unpluggedNote")))));
    }
    showNotices(warnings, list);
    screenSwitch.querySelector("input").checked = status?.state !== "off";
  };

  const element = h("div.stack",
    head(t("homeTitle"), t("homeSub"), h("button.btn", { onclick: () => { action("repair"); toast(t("repairDone")); } }, icon("repair"), t("repair"))),
    warnings,
    h("div.home",
      stage,
      h("div.stack",
        h("div.card", h("div.row", h("h2.grow", t("brightness")), brightnessText), brightness),
        h("div.card", setting(t("screen"), t("screenNote"), screenSwitch)),
        h("div.card", h("h2", t("activeTheme")), h("div.row", themeName, h("button.btn.small", { onclick: () => go("themes") }, t("change")))),
        h("div.card", h("h2", t("health")), h("div.health",
          h("div", h("small", t("frameTime")), frame),
          h("div", h("small", t("port")), port),
          h("div", h("small", t("reconnects")), reconnects),
          h("div", h("small", t("healed")), healed))),
      ),
      h("div.tiles", tiles.map((tile) => tile.element)),
    ));
  update(s);
  return { element, update };
}

// ---- temas ----
// la dirección de una miniatura no cambia al editar el tema: sin esto la vista puede reusar la imagen vieja
let thumbVersion = 0;

function themesPage() {
  thumbVersion += 1;
  const grid = h("div.gallery");
  const actions = h("div.toolbar");
  const picked = new Map();
  let selecting = false;
  let hidden = [];
  const thumb = (id) => image(`thumb?id=${encodeURIComponent(id)}&v=${thumbVersion}`);
  const paintActions = () => {
    if (selecting) {
      actions.replaceChildren(h("div.row",
        h("button.btn", { onclick: stopSelecting }, t("cancel")),
        h("button.btn.danger", { disabled: picked.size === 0, onclick: removePicked }, icon("trash"), t("deleteSelected", { count: picked.size }))));
      return;
    }
    actions.replaceChildren(
      h("div.row",
        h("button.btn", { onclick: startSelecting }, icon("check"), t("select")),
        hidden.length > 0 && h("button.btn", { onclick: restoreDialog }, icon("undo"), t("restoreThemes", { count: hidden.length }))),
      h("div.row",
        h("button.btn", { onclick: () => action("open_themes") }, t("folder")),
        h("button.btn.primary", { onclick: create }, icon("plus"), t("newTheme"))));
  };
  const startSelecting = () => {
    selecting = true;
    grid.classList.add("selecting");
    paintActions();
  };
  const stopSelecting = () => {
    selecting = false;
    picked.clear();
    grid.classList.remove("selecting");
    grid.querySelectorAll(".picked").forEach((card) => card.classList.remove("picked"));
    paintActions();
  };
  const pick = (theme, card) => {
    if (!picked.delete(theme.id)) picked.set(theme.id, theme);
    card.classList.toggle("picked", picked.has(theme.id));
    paintActions();
  };
  const load = async () => {
    const [list, gone] = await Promise.all([api("themes"), api("themes/hidden")]);
    hidden = gone;
    paintActions();
    const active = app.state.config.panel.theme;
    grid.replaceChildren(...list.map((theme) => {
      const inUse = theme.id === active;
      const card = h(`div.theme-card${inUse ? ".on" : ""}`, { onclick: () => { if (selecting && !inUse) pick(theme, card); } },
        h("span.tick", icon("check")),
        h("img", { src: thumb(theme.id), alt: theme.name, ondblclick: () => { if (!selecting) editTheme(theme); } }),
        h("div.theme-foot",
          h("b", theme.name),
          h("button.btn.small.icon", { title: t("edit"), onclick: () => editTheme(theme) }, icon("edit")),
          theme.edited && h("button.btn.small.icon", { title: t("themeRevert"), onclick: () => revertTheme(theme) }, icon("undo")),
          !inUse && h("button.btn.small.icon", { title: t("remove"), onclick: () => removeTheme(theme) }, icon("trash")),
          inUse ? h("span.chip.good", t("inUse")) : h("button.btn.small", { onclick: () => useTheme(theme.id) }, t("use"))));
      return card;
    }));
  };
  const useTheme = async (id) => {
    await setConfig((c) => { c.panel.theme = id; });
    load();
  };
  const editTheme = (theme) => openEditor(theme.id);
  const removeTheme = async (theme) => {
    const note = !theme.builtin ? "confirmDeleteNote" : theme.edited ? "confirmHideEditedNote" : "confirmHideNote";
    if (!(await confirmBox(t("confirmDelete", { name: theme.name }), t(note), t("remove"), true))) return;
    try {
      await post(`theme/delete?id=${encodeURIComponent(theme.id)}`);
      if (theme.builtin) await setConfig((c) => { if (!c.hidden_themes.includes(theme.id)) c.hidden_themes.push(theme.id); });
    } catch (e) {
      toast(e.message);
    }
    load();
  };
  const removePicked = async () => {
    const themes = [...picked.values()];
    const title = themes.length === 1 ? t("confirmDelete", { name: themes[0].name }) : t("confirmDeleteMany", { count: themes.length });
    const note = themes.some((theme) => !theme.builtin) ? "confirmDeleteManyNote" : "confirmHideManyNote";
    if (!(await confirmBox(title, t(note), t("remove"), true))) return;
    const removed = [];
    for (const theme of themes) {
      try {
        await post(`theme/delete?id=${encodeURIComponent(theme.id)}`);
        if (theme.builtin) removed.push(theme.id);
      } catch (e) {
        toast(e.message);
      }
    }
    await setConfig((c) => { c.hidden_themes = [...new Set([...c.hidden_themes, ...removed])]; });
    stopSelecting();
    load();
  };
  const revertTheme = async (theme) => {
    if (!(await confirmBox(t("themeRevertAsk", { name: theme.name }), t("themeRevertNote"), t("themeRevert"), true))) return;
    await post(`theme/revert?id=${encodeURIComponent(theme.id)}`).catch((e) => toast(e.message));
    thumbVersion += 1;
    load();
  };
  // solo devuelve los elegidos: lo que ya está en la galería no se toca
  const restoreDialog = () => {
    const chosen = new Set();
    const accept = h("button.btn.primary", {
      onclick: async () => {
        closeDialog();
        await setConfig((c) => { c.hidden_themes = c.hidden_themes.filter((id) => !chosen.has(id)); });
        toast(t("themesRestored"));
        load();
      },
    });
    const refresh = () => {
      accept.disabled = chosen.size === 0;
      accept.textContent = t("restorePick", { count: chosen.size });
    };
    const cards = hidden.map((theme) => {
      const card = h("button", {
        title: theme.name,
        onclick: () => {
          if (!chosen.delete(theme.id)) chosen.add(theme.id);
          card.classList.toggle("on", chosen.has(theme.id));
          refresh();
        },
      }, h("img", { src: thumb(theme.id), alt: theme.name }), h("span", theme.name));
      return card;
    });
    const all = () => {
      hidden.forEach((theme) => chosen.add(theme.id));
      cards.forEach((card) => card.classList.add("on"));
      refresh();
    };
    refresh();
    dialog(h("div.dialog",
      h("h2", t("restoreTitle")),
      h("p", t("restoreNote")),
      h("div.mini-gallery.restore-list", cards),
      h("div.actions",
        h("button.btn", { onclick: all }, t("selectAll")),
        h("button.btn", { onclick: closeDialog }, t("cancel")),
        accept)));
  };
  const create = async () => {
    const name = await askText(t("themeName"), "", t("create"));
    if (!name) return;
    const { id } = await post(`theme/duplicate?id=signal&name=${encodeURIComponent(name)}`);
    openEditor(id);
  };
  paintActions();
  load();
  return { element: h("div", head(t("themesTitle"), t("themesSub")), actions, grid) };
}

// ---- sensores ----
const GROUPS = [
  ["groupCpu", ["cpu_usage", "cpu_temp"], "sourceCpu"],
  ["groupGpu", ["gpu_usage", "gpu_temp", "gpu_memory_used", "gpu_memory_percent", "gpu_fan", "gpu_clock", "fps"], "sourceGpu"],
  ["groupMemory", ["ram_percent", "ram_used", "swap_percent", "disk_percent", "disk_used", "disk_free"]],
  ["groupNetwork", ["net_down", "net_up", "net_down_total", "net_up_total", "ping"]],
  ["groupSystem", ["time", "date", "weekday", "uptime"]],
  ["groupWeather", ["weather_temp", "weather_feels", "weather_humidity", "weather_text"]],
];

function sensorsPage() {
  const cells = {};
  const warnings = h("div.stack");
  const hints = h("div.stack");
  const cards = GROUPS.map(([title, metrics, source]) => h("div.card", h("h2", t(title)), metrics.map((metric) => {
    cells[metric] = h("b", "--");
    return h("div.reading", h("span", metricName(metric)), cells[metric]);
  }), source && h("p.card-note", t(source, { tool: fpsToolName() }))));
  const update = (state) => {
    for (const [metric, cell] of Object.entries(cells)) cell.textContent = show(metric, value(state.status, metric));
    showNotices(warnings, notices());
    const list = [];
    if (state.status?.gpu === "unavailable") list.push(h("p.muted.small", t("gpuMissing")));
    if (state.status?.cpu_temp === "unavailable") list.push(h("p.muted.small", t("cpuUnsupported")));
    hints.replaceChildren(...list);
  };
  const element = h("div.stack", head(t("sensorsTitle"), t("sensorsSub")), warnings, h("div.sensor-grid", cards), hints);
  update(app.state);
  return { element, update };
}

// ---- alertas ----
function limit(name, current, apply) {
  const label = h("span.muted", current ? `${current} °C` : t("alertOff"));
  const input = slider(0, 110, current, (v) => { label.textContent = v ? `${v} °C` : t("alertOff"); }, apply);
  return h("div", h("div.row", h("b.grow", name), label), input);
}

function alertsPage() {
  const c = app.state.config;
  const url = h("input", { type: "text", value: c.alerts.battery_url, onchange: () => setConfig((x) => { x.alerts.battery_url = url.value.trim(); }) });
  return {
    element: h("div.stack",
      head(t("alertsTitle"), t("alertsSub"), h("button.btn", { onclick: () => action("test_alert") }, t("testAlerts"))),
      h("div.card", h("h2", t("tempAlerts")), h("p.card-note", t("limitNote")), h("div.stack", { style: "margin-top:12px" },
        limit(t("cpuLimit"), c.alerts.cpu_temp, (v) => setConfig((x) => { x.alerts.cpu_temp = v; })),
        limit(t("gpuLimit"), c.alerts.gpu_temp, (v) => setConfig((x) => { x.alerts.gpu_temp = v; })))),
      h("div.card",
        setting(t("batteries"), t("batteriesNote"), toggle(c.alerts.battery, (on) => setConfig((x) => { x.alerts.battery = on; }))),
        setting(t("batteryUrl"), null, h("div", { style: "width:280px" }, url)))),
  };
}

// ---- clima ----
function weatherPage() {
  const c = app.state.config;
  const results = h("div.stack");
  const current = h("span.muted", c.weather.place || t("none"));
  const field = h("input.grow", { type: "text", placeholder: t("placeHint"), onkeydown: (e) => { if (e.key === "Enter") search(); } });
  const search = async () => {
    results.replaceChildren();
    const places = await api(`places?q=${encodeURIComponent(field.value)}`).catch((e) => { toast(e.message); return []; });
    if (!places.length) {
      results.append(h("p.muted.small", t("noPlace")));
      return;
    }
    results.replaceChildren(...places.map((place) => {
      const label = [place.name, place.admin1, place.country].filter(Boolean).join(", ");
      return h("button.btn", {
        style: "justify-content:flex-start",
        onclick: () => {
          setConfig((x) => {
            Object.assign(x.weather, { enabled: true, place: label, latitude: place.latitude, longitude: place.longitude });
          });
          current.textContent = label;
          results.replaceChildren();
          draw();
        },
      }, label);
    }));
  };
  return {
    element: h("div.stack",
      head(t("weatherTitle"), t("weatherSub")),
      h("div.card",
        setting(t("weatherOn"), t("weatherTip"), toggle(c.weather.enabled, (on) => setConfig((x) => { x.weather.enabled = on; }))),
        setting(t("currentPlace"), null, current),
        setting(t("fahrenheit"), null, toggle(c.weather.fahrenheit, (on) => setConfig((x) => { x.weather.fahrenheit = on; })))),
      h("div.card", h("h2", t("place")), h("div.row", field, h("button.btn.primary", { onclick: search }, t("search"))), h("div.stack", { style: "margin-top:10px" }, results))),
  };
}

// ---- juegos ----
// se elige por el título de la ventana: el nombre del .exe casi nadie lo conoce
async function pickProgram(add) {
  const ignored = app.state.config.fps.ignored;
  const programs = (await api("programs").catch(() => [])).filter((program) => !ignored.includes(program.exe));
  dialog(h("div.dialog",
    h("h2", t("pickProgram")),
    h("p", t("pickProgramNote")),
    programs.length
      ? h("div.program-list", programs.map((program) => h("button", { onclick: () => { closeDialog(); add(program.exe); } },
        h("b", program.title), h("span", program.exe))))
      : h("p", t("noPrograms")),
    h("div.actions", h("button.btn", { onclick: closeDialog }, t("cancel")))));
}

function gamesPage() {
  const list = h("div.row", { style: "flex-wrap:wrap" });
  const foreground = h("b", "--");
  const fps = h("b", "--");
  const drawList = () => list.replaceChildren(...app.state.config.fps.ignored.map((name) =>
    h("span.chip", name, h("button", { onclick: () => removeProgram(name) }, icon("close")))));
  const add = async (name) => {
    const clean = name.trim().toLowerCase();
    if (!clean || app.state.config.fps.ignored.includes(clean)) return;
    await setConfig((x) => { x.fps.ignored.push(clean); });
    drawList();
  };
  const removeProgram = async (name) => {
    await setConfig((x) => { x.fps.ignored = x.fps.ignored.filter((n) => n !== name); });
    drawList();
  };
  const field = h("input.grow", {
    type: "text",
    placeholder: t("addProgram"),
    onkeydown: (e) => { if (e.key === "Enter") { add(field.value); field.value = ""; } },
  });
  const warnings = h("div.stack");
  const update = (state) => {
    foreground.textContent = state.status?.foreground || "--";
    fps.textContent = show("fps", value(state.status, "fps"));
    showNotices(warnings, notices());
  };
  drawList();
  update(app.state);
  return {
    element: h("div.stack",
      head(t("gamesTitle"), t("gamesSub")),
      warnings,
      h("div.card",
        setting(t("foreground"), null, foreground),
        setting("FPS", null, fps),
        setting(t("fpsTool"), t("fpsToolNote"), h("span.muted", capitalized(fpsToolName()))),
        setting(t("countGenerated"), t("countGeneratedNote"), toggle(app.state.config.fps.count_generated, (on) => setConfig((x) => { x.fps.count_generated = on; })))),
      h("div.card", h("h2", t("ignored")), h("p.card-note", t("ignoredNote")),
        h("div.row", { style: "margin:10px 0" },
          h("button.btn.primary", { onclick: () => pickProgram(add) }, t("pickProgram")),
          field, h("button.btn", { onclick: () => { add(field.value); field.value = ""; } }, t("add"))),
        list)),
    update,
  };
}

// ---- ajustes ----
function helperRow(state) {
  const remove = h("button.btn.small.danger", { onclick: () => helperDialog(true) }, t("helperRemove"));
  if (state.helper_outdated) {
    return setting(t("helper"), t("helperOutdatedNote"), h("div.row",
      h("button.btn.primary.small", { onclick: () => helperDialog(false) }, t("helperReinstall")), remove));
  }
  const usedTool = `${t("helperToolNote", { tool: fpsToolName() })} ${t(state.pawnio ? "helperCpuNote" : "helperCpuMissingNote")}`;
  return setting(t("helper"), state.helper ? `${t("helperOnNote")} ${usedTool}` : t("helperOffNote"), state.helper
    ? h("div.row", h("span.chip.good", icon("check"), t("helperActive")), remove)
    : h("button.btn.primary.small", { onclick: () => helperDialog(false) }, t("helperEnable")));
}

function settingsPage() {
  const s = app.state;
  const c = s.config;
  const helperKey = (state) => `${state.helper}${state.helper_outdated}`;
  let shownHelper = helperKey(s);
  const helper = h("div.setting-wrap", helperRow(s));
  const port = h("select", { onchange: () => setConfig((x) => { x.panel.port = port.value; }) },
    h("option", { value: "auto" }, t("auto")),
    s.ports.map((p) => h("option", { value: p.name }, p.name)));
  port.value = c.panel.port;
  const network = h("select", { onchange: () => setConfig((x) => { x.network = network.value; }) },
    h("option", { value: "auto" }, t("auto")),
    s.interfaces.map((name) => h("option", { value: name }, name)));
  network.value = s.interfaces.includes(c.network) ? c.network : "auto";
  const ping = h("input", { type: "text", value: c.ping_host, onchange: () => setConfig((x) => { x.ping_host = ping.value.trim(); }) });
  const days = h("select", { onchange: () => setConfig((x) => { x.maintenance.deghost_days = +days.value; }) },
    h("option", { value: 0 }, t("never")),
    [7, 14, 30].map((n) => h("option", { value: n }, t("everyDays", { n }))));
  days.value = c.maintenance.deghost_days;
  const hardReset = async () => {
    if (await confirmBox(t("hardResetAsk"), t("hardResetNote"), t("restart"), true)) action("hard_reset");
  };
  return {
    element: h("div.stack",
      head(t("settingsTitle")),
      h("div.card", h("h2", t("general")),
        setting(t("language"), null, segments([["spanish", "Español"], ["english", "English"]], c.language, async (id) => { await setConfig((x) => { x.language = id; }); draw(); })),
        setting(t("appearance"), null, segments([["system", t("system")], ["dark", t("dark")], ["light", t("light")]], c.appearance, (id) => setConfig((x) => { x.appearance = id; }))),
        setting(t("clock24"), null, toggle(c.clock_24h, (on) => setConfig((x) => { x.clock_24h = on; }))),
        setting(t("startup"), t("startupNote"), toggle(c.start_with_windows, (on) => setConfig((x) => { x.start_with_windows = on; }))),
        helper),
      h("div.card", h("h2", t("panel")),
        setting(t("portChoice"), null, port),
        setting(t("flip"), t("flipNote"), toggle(c.panel.flipped, (on) => setConfig((x) => { x.panel.flipped = on; }))),
        setting(t("offIdle"), t("offIdleNote"), toggle(c.panel.off_when_idle, (on) => setConfig((x) => { x.panel.off_when_idle = on; })))),
      h("div.card", h("h2", t("network")),
        setting(t("interface"), null, network),
        setting(t("pingHost"), null, ping)),
      h("div.card", h("h2", t("maintenance")),
        setting(t("deghost"), t("deghostNote"), h("button.btn", { onclick: () => action("deghost") }, t("deghostNow"))),
        setting(t("deghostDays"), null, days),
        setting(t("hardReset"), t("hardResetNote"), h("button.btn.danger", { onclick: hardReset }, t("restart")))),
      h("div.card", h("h2", t("about")),
        setting(`PulseLCD ${s.version}`, t("aboutNote"), h("div.row",
          h("button.btn", { onclick: () => action("repository") }, t("github")),
          h("button.btn", { onclick: () => action("kofi") }, icon("heart"), t("kofi")))),
        setting(t("checkUpdates"), t("checkUpdatesNote"), toggle(c.check_updates, async (on) => { await setConfig((x) => { x.check_updates = on; }); checkForUpdate(); })),
        setting(t("files"), null, h("button.btn", { onclick: () => action("open_log") }, t("openLog"))),
        setting(t("quit"), t("quitNote"), h("button.btn.danger", { onclick: () => action("quit") }, t("quit"))))),
    update: (state) => {
      if (helperKey(state) !== shownHelper) {
        shownHelper = helperKey(state);
        helper.replaceChildren(helperRow(state));
      }
    },
  };
}

// ---- bienvenida ----
const WELCOME_THEMES = 6;

export function welcome() {
  let step = 0;
  const finish = async () => {
    await setConfig((c) => { c.welcomed = true; });
    closeDialog();
    draw();
  };
  const paint = async () => {
    const steps = h("div.steps", [0, 1, 2].map((i) => h(`i${i <= step ? ".on" : ""}`)));
    const body = [];
    if (step === 0) {
      const port = app.state.status?.port;
      body.push(h("h2", t("welcomeTitle")), h("p.muted", port ? t("welcomeFound", { port }) : t("welcomeSearch")));
    } else if (step === 1) {
      const list = (await api("themes")).filter((x) => x.builtin).slice(0, WELCOME_THEMES);
      const gallery = h("div.mini-gallery");
      const fill = () => gallery.replaceChildren(...list.map((theme) => h(`button${app.state.config.panel.theme === theme.id ? ".on" : ""}`, {
        onclick: async () => { await setConfig((c) => { c.panel.theme = theme.id; }); fill(); },
      }, h("img", { src: image(`thumb?id=${theme.id}`), alt: theme.name }))));
      fill();
      body.push(h("h2", t("welcomeTheme")), h("p.muted", t("welcomeThemeNote")), gallery);
    } else {
      body.push(h("h2", t("welcomeStart")),
        setting(t("startup"), t("startupNote"), toggle(app.state.config.start_with_windows, (on) => setConfig((c) => { c.start_with_windows = on; }))),
        setting(t("checkUpdates"), t("checkUpdatesNote"), toggle(app.state.config.check_updates, (on) => setConfig((c) => { c.check_updates = on; }))),
        setting(t("helper"), t("helperOffNote"), app.state.helper ? h("span.chip.good", icon("check")) : h("button.btn.small", { onclick: async () => { await finish(); helperDialog(false); } }, t("helperEnable"))));
    }
    dialog(h("div.welcome", body, steps,
      h("div.actions",
        step > 0 && h("button.btn", { onclick: () => { step -= 1; paint(); } }, t("back")),
        step < 2
          ? h("button.btn.primary", { onclick: () => { step += 1; paint(); } }, t("next"))
          : h("button.btn.primary", { onclick: finish }, t("finish")))));
  };
  paint();
}

let welcomed = false;

export function renderPage(page) {
  const pages = { home: homePage, themes: themesPage, sensors: sensorsPage, alerts: alertsPage, weather: weatherPage, games: gamesPage, settings: settingsPage };
  if (!app.state.config.welcomed && !welcomed) {
    welcomed = true;
    setTimeout(welcome);
  }
  return pages[page]();
}
