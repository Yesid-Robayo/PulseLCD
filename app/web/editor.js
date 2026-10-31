// Editor visual de temas. La imagen la dibuja el mismo código que el panel: lo que se ve aquí es exactamente lo que saldrá.
import { api, app, confirmBox, draw, h, icon, post, setConfig, toast } from "./app.js";
import { METRIC_IDS, metricName, t } from "./strings.js";

const SNAP = 4;
const MARGIN = 28;
const MIN_SIZE = 8;
const HISTORY_LIMIT = 100;

const FRESH = {
  label: () => ({ w: 140, h: 28, type: "label", text: t("addLabel"), font: "seguisb.ttf", size: 18, color: "#ffffff", align: "left" }),
  value: () => ({ w: 140, h: 30, type: "value", metric: "cpu_usage", unit: true, font: "segoeuib.ttf", size: 22, color: "#ffffff", align: "left" }),
  bar: () => ({ w: 160, h: 10, type: "bar", metric: "cpu_usage", color: "#2ee6b6", track: "#16323a", radius: 5 }),
  ring: () => ({ w: 110, h: 110, type: "ring", metric: "cpu_usage", color: "#2ee6b6", track: "#16323a", thickness: 10, start: 225, sweep: 270 }),
  graph: () => ({ w: 220, h: 60, type: "graph", metric: "cpu_usage", color: "#2ee6b6", history: 60, fill: true }),
  panel: () => ({ w: 160, h: 90, type: "panel", color: "#ffffff", opacity: 6, radius: 14 }),
};

const KIND_ICON = { label: "text", value: "value", bar: "bar", ring: "ring", graph: "graph", panel: "panel" };

function describe(w) {
  if (w.type === "label") return w.text || t("addLabel");
  if (w.type === "panel") return t("addPanel");
  const kind = { value: "addValue", bar: "addBar", ring: "addRing", graph: "addGraph" }[w.type];
  return `${metricName(w.metric)} · ${t(kind)}`;
}

export async function openEditor(id) {
  const theme = await api(`theme?id=${encodeURIComponent(id)}`);
  const fonts = await api(`fonts?id=${encodeURIComponent(id)}`);
  const editor = new Editor(id, theme, fonts);
  app.locked = true;
  app.update = null;
  document.body.classList.add("editing");
  document.getElementById("page").replaceChildren(editor.element);
  editor.layout();
  editor.refresh();
}

class Editor {
  constructor(id, theme, fonts) {
    this.id = id;
    this.theme = theme;
    this.fonts = fonts;
    this.selected = -1;
    this.scale = 1;
    this.past = [];
    this.future = [];
    this.dirty = false;
    this.live = false;
    this.pending = false;
    this.rendering = false;

    this.picture = h("img", { alt: "", draggable: "false" });
    this.boxes = h("div");
    this.canvas = h("div.canvas", {
      onpointerdown: (e) => { if (e.target === this.picture) this.select(-1); },
    }, this.picture, this.boxes);
    this.zone = h("div.canvas-zone", this.canvas);
    this.layers = h("div.stack", { style: "gap:2px" });
    this.properties = h("div");
    this.undoButton = h("button.btn.small.icon", { title: t("undo"), onclick: () => this.undo() }, icon("undo"));
    this.redoButton = h("button.btn.small.icon", { title: t("redo"), onclick: () => this.redo() }, icon("redo"));
    this.nameField = h("input.name", {
      type: "text",
      value: theme.name,
      maxlength: 40,
      onchange: () => { this.theme.name = this.nameField.value.trim() || this.theme.name; this.dirty = true; },
    });

    const adds = Object.keys(FRESH).map((kind) => h("button.btn.small", {
      onclick: () => this.add(kind),
    }, icon(KIND_ICON[kind]), t({ label: "addLabel", value: "addValue", bar: "addBar", ring: "addRing", graph: "addGraph", panel: "addPanel" }[kind])));

    const liveInput = h("input", { type: "checkbox", onchange: () => { this.live = liveInput.checked; this.refresh(); } });

    this.element = h("div.editor",
      h("div.editor-bar",
        h("button.btn.small.icon", { title: t("close"), onclick: () => this.leave() }, icon("back")),
        this.nameField,
        h("div.bar-sep"),
        this.undoButton,
        this.redoButton,
        h("div.grow"),
        h("span.muted.small", t("liveData")),
        h("label.switch", liveInput, h("i")),
        h("button.btn.small", { onclick: () => this.save(false) }, t("save")),
        h("button.btn.small.primary", { onclick: () => this.save(true) }, t("saveUse"))),
      h("div.editor-side", h("h2", t("add")), h("div.add-grid", adds), h("h2", { style: "margin-top:12px" }, t("layers")), this.layers),
      this.zone,
      h("div.editor-side", this.properties));

    this.keys = (e) => this.key(e);
    this.resize = () => this.layout();
    window.addEventListener("keydown", this.keys);
    window.addEventListener("resize", this.resize);
    this.drawLayers();
    this.drawProperties();
  }

  size() {
    return this.theme.orientation === "portrait" ? [320, 480] : [480, 320];
  }

  // el lienzo ocupa todo lo que deja la ventana
  layout() {
    const [w, h_] = this.size();
    const zone = this.zone.getBoundingClientRect();
    this.scale = Math.max(0.5, Math.min((zone.width - MARGIN) / w, (zone.height - MARGIN) / h_));
    this.canvas.style.width = `${w * this.scale}px`;
    this.canvas.style.height = `${h_ * this.scale}px`;
    this.drawBoxes();
  }

  // ---- imagen ----
  async refresh() {
    if (this.rendering) {
      this.pending = true;
      return;
    }
    this.rendering = true;
    try {
      const response = await fetch(`/api/preview?id=${encodeURIComponent(this.id)}${this.live ? "&live=1" : ""}`, {
        method: "POST",
        body: JSON.stringify(this.theme),
      });
      if (response.ok) {
        const old = this.picture.src;
        this.picture.src = URL.createObjectURL(await response.blob());
        if (old.startsWith("blob:")) URL.revokeObjectURL(old);
      } else {
        toast((await response.json()).error);
      }
    } finally {
      this.rendering = false;
      if (this.pending) {
        this.pending = false;
        this.refresh();
      }
    }
  }

  // ---- historial de cambios ----
  remember() {
    this.past.push(JSON.stringify(this.theme));
    if (this.past.length > HISTORY_LIMIT) this.past.shift();
    this.future = [];
    this.dirty = true;
  }

  restore(from, to) {
    if (!from.length) return;
    to.push(JSON.stringify(this.theme));
    this.theme = JSON.parse(from.pop());
    this.selected = Math.min(this.selected, this.theme.widgets.length - 1);
    this.dirty = true;
    this.redrawAll();
  }

  undo() { this.restore(this.past, this.future); }
  redo() { this.restore(this.future, this.past); }

  redrawAll() {
    this.layout();
    this.drawLayers();
    this.drawProperties();
    this.refresh();
  }

  // ---- selección ----
  select(index) {
    this.selected = index;
    this.drawBoxes();
    this.drawLayers();
    this.drawProperties();
  }

  add(kind) {
    this.remember();
    const [w, h_] = this.size();
    const fresh = FRESH[kind]();
    this.theme.widgets.push({ x: Math.round((w - fresh.w) / 2), y: Math.round((h_ - fresh.h) / 2), ...fresh });
    this.selected = this.theme.widgets.length - 1;
    this.redrawAll();
  }

  duplicateSelected() {
    const widget = this.theme.widgets[this.selected];
    if (!widget) return;
    this.remember();
    this.theme.widgets.push({ ...structuredClone(widget), x: widget.x + 12, y: widget.y + 12 });
    this.selected = this.theme.widgets.length - 1;
    this.redrawAll();
  }

  removeSelected() {
    if (!this.theme.widgets[this.selected]) return;
    this.remember();
    this.theme.widgets.splice(this.selected, 1);
    this.selected = -1;
    this.redrawAll();
  }

  moveLayer(delta) {
    const target = this.selected + delta;
    const list = this.theme.widgets;
    if (this.selected < 0 || target < 0 || target >= list.length) return;
    this.remember();
    [list[this.selected], list[target]] = [list[target], list[this.selected]];
    this.selected = target;
    this.redrawAll();
  }

  // ---- arrastrar y redimensionar ----
  drawBoxes() {
    this.boxes.replaceChildren(...this.theme.widgets.map((w, i) => {
      const box = h(`div.box${i === this.selected ? ".on" : ""}`, {
        style: `left:${w.x * this.scale}px;top:${w.y * this.scale}px;width:${w.w * this.scale}px;height:${w.h * this.scale}px`,
        onpointerdown: (e) => this.grab(e, i, "move"),
      });
      if (i === this.selected) {
        for (const [edge, cx, cy, cursor] of [
          ["nw", 0, 0, "nwse"], ["n", 50, 0, "ns"], ["ne", 100, 0, "nesw"], ["e", 100, 50, "ew"],
          ["se", 100, 100, "nwse"], ["s", 50, 100, "ns"], ["sw", 0, 100, "nesw"], ["w", 0, 50, "ew"],
        ]) {
          box.append(h("div.grip", {
            style: `left:calc(${cx}% - 5px);top:calc(${cy}% - 5px);cursor:${cursor}-resize`,
            onpointerdown: (e) => this.grab(e, i, edge),
          }));
        }
      }
      return box;
    }));
  }

  // líneas a las que pegarse: bordes y centro de la pantalla y de los demás elementos
  snapLines(index) {
    const [w, h_] = this.size();
    const xs = [0, w / 2, w];
    const ys = [0, h_ / 2, h_];
    this.theme.widgets.forEach((o, i) => {
      if (i === index) return;
      xs.push(o.x, o.x + o.w / 2, o.x + o.w);
      ys.push(o.y, o.y + o.h / 2, o.y + o.h);
    });
    return { xs, ys };
  }

  showGuides(x, y) {
    this.boxes.querySelectorAll(".guide").forEach((g) => g.remove());
    const [w, h_] = this.size();
    if (x !== null) this.boxes.append(h("div.guide", { style: `left:${x * this.scale}px;top:0;width:1px;height:${h_ * this.scale}px` }));
    if (y !== null) this.boxes.append(h("div.guide", { style: `left:0;top:${y * this.scale}px;width:${w * this.scale}px;height:1px` }));
  }

  grab(event, index, edge) {
    event.preventDefault();
    event.stopPropagation();
    if (this.selected !== index) this.select(index);
    const widget = this.theme.widgets[index];
    const start = { ...widget };
    const origin = [event.clientX, event.clientY];
    const lines = this.snapLines(index);
    const before = JSON.stringify(this.theme);
    let changed = false;

    const nearest = (values, list) => {
      let best = null;
      for (const value of values) {
        for (const line of list) {
          const gap = line - value;
          if (Math.abs(gap) <= SNAP && (best === null || Math.abs(gap) < Math.abs(best.gap))) best = { gap, line };
        }
      }
      return best;
    };

    const move = (e) => {
      const dx = Math.round((e.clientX - origin[0]) / this.scale);
      const dy = Math.round((e.clientY - origin[1]) / this.scale);
      const free = e.shiftKey;
      let guideX = null;
      let guideY = null;
      if (edge === "move") {
        widget.x = start.x + dx;
        widget.y = start.y + dy;
        if (!free) {
          const sx = nearest([widget.x, widget.x + widget.w / 2, widget.x + widget.w], lines.xs);
          const sy = nearest([widget.y, widget.y + widget.h / 2, widget.y + widget.h], lines.ys);
          if (sx) { widget.x += Math.round(sx.gap); guideX = sx.line; }
          if (sy) { widget.y += Math.round(sy.gap); guideY = sy.line; }
        }
      } else {
        if (edge.includes("e")) widget.w = Math.max(MIN_SIZE, start.w + dx);
        if (edge.includes("s")) widget.h = Math.max(MIN_SIZE, start.h + dy);
        if (edge.includes("w")) {
          widget.w = Math.max(MIN_SIZE, start.w - dx);
          widget.x = start.x + start.w - widget.w;
        }
        if (edge.includes("n")) {
          widget.h = Math.max(MIN_SIZE, start.h - dy);
          widget.y = start.y + start.h - widget.h;
        }
      }
      // los mismos límites que comprueba el tema al cargarse
      const [panelW, panelH] = this.size();
      widget.x = Math.min(panelW - 1, Math.max(1 - panelW, widget.x));
      widget.y = Math.min(panelH - 1, Math.max(1 - panelH, widget.y));
      widget.w = Math.min(panelW * 2, widget.w);
      widget.h = Math.min(panelH * 2, widget.h);
      changed = true;
      this.drawBoxes();
      this.showGuides(guideX, guideY);
      this.drawPosition();
      this.refresh();
    };

    const release = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", release);
      this.showGuides(null, null);
      if (changed) {
        this.past.push(before);
        if (this.past.length > HISTORY_LIMIT) this.past.shift();
        this.future = [];
        this.dirty = true;
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", release);
  }

  key(e) {
    if (e.target.matches("input, select")) return;
    const nudge = e.shiftKey ? 10 : 1;
    const moves = { ArrowLeft: [-nudge, 0], ArrowRight: [nudge, 0], ArrowUp: [0, -nudge], ArrowDown: [0, nudge] };
    const widget = this.theme.widgets[this.selected];
    if (moves[e.key] && widget) {
      e.preventDefault();
      this.remember();
      widget.x += moves[e.key][0];
      widget.y += moves[e.key][1];
      this.drawBoxes();
      this.drawPosition();
      this.refresh();
    } else if (e.key === "Delete") this.removeSelected();
    else if (e.ctrlKey && e.key === "z") this.undo();
    else if (e.ctrlKey && e.key === "y") this.redo();
    else if (e.ctrlKey && e.key === "d") { e.preventDefault(); this.duplicateSelected(); }
    else if (e.key === "Escape") this.select(-1);
  }

  // ---- listas ----
  drawLayers() {
    const items = this.theme.widgets.map((w, i) => h(`button.layer${i === this.selected ? ".on" : ""}`, { onclick: () => this.select(i) },
      icon(KIND_ICON[w.type]), h("span", describe(w))));
    this.layers.replaceChildren(...items.reverse());
  }

  change(apply) {
    this.remember();
    apply();
    this.refresh();
    this.drawBoxes();
  }

  // un control numérico o de texto que guarda al cambiar
  field(label, input) {
    return h("label.field", label, input);
  }

  // se ve al arrastrar: cada movimiento vuelve a encuadrar la vista previa
  slider(widget, key, label, min, max) {
    const paint = () => input.style.setProperty("--fill", `${((widget[key] - min) / (max - min)) * 100}%`);
    const input = h("input", {
      type: "range", min, max, value: widget[key],
      onpointerdown: () => this.remember(),
      oninput: () => { widget[key] = Number(input.value); paint(); this.dirty = true; this.refresh(); },
    });
    paint();
    return this.field(label, input);
  }

  // el archivo guarda enteros en la mayoría de campos: un decimal aquí haría fallar el tema al guardarlo
  number(widget, key, label, min, max, decimals = false) {
    const input = h("input", {
      type: "number", min, max, value: widget[key],
      onchange: () => this.change(() => {
        const value = Number(input.value) || 0;
        widget[key] = Math.min(max, Math.max(min, decimals ? value : Math.round(value)));
        input.value = widget[key];
      }),
    });
    return this.field(label, input);
  }

  color(widget, key, label) {
    const input = h("input", {
      type: "color", value: widget[key],
      oninput: () => { widget[key] = input.value; this.dirty = true; this.refresh(); },
    });
    return h("div.field-inline", label, input);
  }

  check(widget, key, label) {
    const input = h("input", { type: "checkbox", checked: widget[key], onchange: () => this.change(() => { widget[key] = input.checked; }) });
    return h("div.field-inline", label, h("label.switch", input, h("i")));
  }

  metricSelect(widget, numericOnly) {
    const textOnly = ["time", "date", "weekday", "weather_text", "uptime"];
    const ids = numericOnly ? METRIC_IDS.filter((m) => !textOnly.includes(m)) : METRIC_IDS;
    const select = h("select", { onchange: () => this.change(() => { widget.metric = select.value; this.drawLayers(); }) },
      ids.map((m) => h("option", { value: m }, metricName(m))));
    select.value = widget.metric;
    return this.field(t("metric"), select);
  }

  textStyle(widget) {
    const font = h("select", {
      onchange: async () => {
        if (font.value === "__add") {
          const picked = await post(`theme/pick?id=${encodeURIComponent(this.id)}&kind=font`).catch((e) => { toast(e.message); return {}; });
          if (picked.file) {
            this.fonts = await api(`fonts?id=${encodeURIComponent(this.id)}`);
            this.change(() => { widget.font = picked.file; });
            this.drawProperties();
          } else font.value = widget.font;
          return;
        }
        this.change(() => { widget.font = font.value; });
      },
    }, this.fonts.map((f) => h("option", { value: f.file }, f.label)), h("option", { value: "__add" }, t("addFont")));
    font.value = widget.font;
    const alignButtons = h("div.segments");
    for (const [id, label] of [["left", t("left")], ["center", t("center")], ["right", t("right")]]) {
      alignButtons.append(h(`button${widget.align === id ? ".on" : ""}`, {
        onclick: () => { this.change(() => { widget.align = id; }); this.drawProperties(); },
      }, label));
    }
    return [
      this.field(t("font"), font),
      this.number(widget, "size", t("size"), 4, 160, true),
      this.color(widget, "color", t("color")),
      h("div.field", t("align"), alignButtons),
    ];
  }

  drawPosition() {
    const widget = this.theme.widgets[this.selected];
    if (!widget || !this.positionInputs) return;
    for (const key of ["x", "y", "w", "h"]) this.positionInputs[key].value = widget[key];
  }

  backgroundFields() {
    const b = this.theme.background;
    const kinds = h("div.segments");
    const pick = (kind) => this.change(() => {
      if (kind === "color") this.theme.background = { kind, color: "#0b1014" };
      if (kind === "gradient") this.theme.background = { kind, from: "#071018", to: "#0d2a33", angle: 55 };
      this.drawProperties();
    });
    const pickImage = async () => {
      const picked = await post(`theme/pick?id=${encodeURIComponent(this.id)}&kind=image`).catch((e) => { toast(e.message); return {}; });
      if (picked.file) {
        this.change(() => { this.theme.background = { kind: "image", file: picked.file, x: 50, y: 50, zoom: 100 }; });
        this.drawProperties();
      }
    };
    for (const [kind, label] of [["color", t("bgColor")], ["gradient", t("bgGradient")], ["image", t("bgImage")]]) {
      kinds.append(h(`button${b.kind === kind ? ".on" : ""}`, { onclick: () => (kind === "image" ? pickImage() : pick(kind)) }, label));
    }
    const rows = [h("h2", t("background")), h("div.field", kinds)];
    if (b.kind === "color") rows.push(this.color(b, "color", t("bgColor")));
    if (b.kind === "gradient") rows.push(this.color(b, "from", t("from")), this.color(b, "to", t("to")), this.number(b, "angle", t("angle"), 0, 360));
    if (b.kind === "image") {
      rows.push(h("p.muted.small", b.file), h("button.btn.small", { onclick: pickImage }, t("chooseImage")),
        h("h2", t("framing")),
        this.slider(b, "x", t("frameX"), 0, 100),
        this.slider(b, "y", t("frameY"), 0, 100),
        this.slider(b, "zoom", t("frameZoom"), 100, 400));
    }
    return rows;
  }

  drawProperties() {
    this.positionInputs = null;
    const widget = this.theme.widgets[this.selected];
    if (!widget) {
      this.properties.replaceChildren(...this.backgroundFields(), h("p.muted.small", { style: "margin-top:14px" }, t("noSelection")));
      return;
    }
    const rows = [h("h2", describe(widget))];
    this.positionInputs = {};
    const [panelW, panelH] = this.size();
    const limits = { x: [1 - panelW, panelW - 1], y: [1 - panelH, panelH - 1], w: [MIN_SIZE, panelW * 2], h: [MIN_SIZE, panelH * 2] };
    const position = h("div.field-row", ["x", "y", "w", "h"].map((key) => {
      const field = this.number(widget, key, key.toUpperCase(), ...limits[key]);
      this.positionInputs[key] = field.querySelector("input");
      return field;
    }));
    switch (widget.type) {
      case "label": {
        const input = h("input", {
          type: "text", value: widget.text, maxlength: 64,
          onchange: () => this.change(() => { widget.text = input.value; this.drawLayers(); }),
        });
        rows.push(this.field(t("text"), input), ...this.textStyle(widget));
        break;
      }
      case "value":
        rows.push(this.metricSelect(widget, false), this.check(widget, "unit", t("unit")), ...this.textStyle(widget));
        break;
      case "bar":
        rows.push(this.metricSelect(widget, true), this.color(widget, "color", t("color")), this.color(widget, "track", t("track")), this.number(widget, "radius", t("radius"), 0, 40));
        break;
      case "ring":
        rows.push(this.metricSelect(widget, true), this.color(widget, "color", t("color")), this.color(widget, "track", t("track")),
          this.number(widget, "thickness", t("thickness"), 1, 80),
          h("div.field-row", this.number(widget, "start", t("start"), 0, 359), this.number(widget, "sweep", t("sweep"), 1, 360)));
        break;
      case "graph":
        rows.push(this.metricSelect(widget, true), this.color(widget, "color", t("color")), this.number(widget, "history", t("history"), 2, 240), this.check(widget, "fill", t("fill")));
        break;
      case "panel":
        rows.push(this.color(widget, "color", t("color")), this.number(widget, "opacity", t("opacity"), 0, 100), this.number(widget, "radius", t("radius"), 0, 40));
        break;
    }
    rows.push(h("h2", { style: "margin-top:6px" }, t("position")), position,
      h("div.row",
        h("button.btn.small.icon", { title: t("moveUp"), onclick: () => this.moveLayer(1) }, icon("up")),
        h("button.btn.small.icon", { title: t("moveDown"), onclick: () => this.moveLayer(-1) }, icon("down")),
        h("button.btn.small.icon", { title: t("duplicateItem"), onclick: () => this.duplicateSelected() }, icon("copy")),
        h("div.grow"),
        h("button.btn.small.danger", { onclick: () => this.removeSelected() }, t("deleteItem"))));
    this.properties.replaceChildren(...rows);
  }

  // ---- guardar y salir ----
  async save(use) {
    try {
      await post(`theme/save?id=${encodeURIComponent(this.id)}`, this.theme);
      this.dirty = false;
      toast(t("editorSaved"));
      if (use) await setConfig((c) => { c.panel.theme = this.id; });
    } catch (e) {
      toast(e.message);
    }
  }

  async leave() {
    if (this.dirty && !(await confirmBox(t("unsaved"), null, t("leave")))) return;
    window.removeEventListener("keydown", this.keys);
    window.removeEventListener("resize", this.resize);
    if (this.picture?.src.startsWith("blob:")) URL.revokeObjectURL(this.picture.src);
    document.body.classList.remove("editing");
    app.locked = false;
    app.page = "themes";
    draw();
  }
}
