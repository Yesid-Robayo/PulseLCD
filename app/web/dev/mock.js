// Sustituye al servidor: responde a /api/ con datos de ejemplo. Parámetros: ?page=themes&theme=light&lang=english&click=Elegir&update=0.2.0&fast=1&noweather=1
(() => {
  const params = new URLSearchParams(location.search);
  const theme = (id, name, builtin, edited = false) => ({ id, name, builtin, edited });
  const list = [theme("pulse", "Pulse", true), theme("signal", "Signal", true, true), theme("arcade", "Arcade", true), theme("tierra", "Tierra", false)];
  const n = (number) => ({ number });
  const x = (text) => ({ text });
  const readings = {
    cpu_usage: n(23), cpu_temp: n(52), gpu_usage: n(68), gpu_temp: n(61), gpu_memory_used: n(7240), gpu_memory_percent: n(45),
    gpu_fan: n(38), gpu_clock: n(2.68), fps: n(144), ram_percent: n(49), ram_used: n(31), swap_percent: n(12), disk_percent: n(63),
    disk_used: n(1180), disk_free: n(680), net_down: n(4200000), net_up: n(380000), net_down_total: n(52e9), net_up_total: n(6e9),
    ping: n(14), uptime: n(190800), time: x("10:36 AM"), date: x("31 oct 2026"), weekday: x("Sábado"), weather_temp: n(17),
    weather_feels: n(15), weather_humidity: n(68), weather_text: x("Nublado"),
  };
  const state = {
    version: "0.1.0",
    connected: params.get("agent") !== "0",
    frame: 1,
    helper: params.get("helper") !== "0",
    helper_outdated: params.get("outdated") === "1",
    fps_tool: params.get("tool") ?? "amd_driver",
    pawnio: true,
    fast_startup: params.get("fast") === "1",
    theme_needs_weather: params.get("noweather") === "1",
    status: {
      state: params.get("state") ?? "healthy", port: "COM5", readings: { values: readings }, cpu_temp: params.get("helper") === "0" ? "needs_helper" : "ok",
      gpu: "ok", fps: "ok", foreground: "eldenring.exe", reconnects: 1, healed: 0, frame_ms: 2400, missing_secs: +(params.get("missing") ?? 0),
    },
    config: {
      language: params.get("lang") ?? "spanish", appearance: params.get("theme") ?? "dark", clock_24h: false, start_with_windows: true, check_updates: params.has("update"),
      welcomed: params.get("welcome") !== "1", hidden_themes: params.has("hidden") ? ["mars", "reef", "blocks"] : [],
      panel: { port: "auto", brightness: 53, flipped: true, theme: "pulse", off_when_idle: true },
      network: "auto", ping_host: "1.1.1.1",
      alerts: { cpu_temp: 90, gpu_temp: 85, battery: true, battery_url: "http://127.0.0.1:8765/v1/status" },
      weather: { enabled: true, place: "Bogotá, Colombia", latitude: 4.6, longitude: -74.1, fahrenheit: false },
      fps: { ignored: ["wallpaper64.exe", "obs64.exe"], count_generated: true },
      maintenance: { deghost_days: 7, last_deghost: 0 },
    },
    ports: [{ name: "COM3", panel: false }, { name: "COM5", panel: true }],
    interfaces: ["Ethernet", "Wi-Fi"],
  };
  const fonts = [{ file: "segoeui.ttf", label: "Segoe UI" }, { file: "seguisb.ttf", label: "Segoe UI Semibold" }, { file: "segoeuib.ttf", label: "Segoe UI Bold" }, { file: "segoeuil.ttf", label: "Segoe UI Light" }, { file: "segoeuisl.ttf", label: "Segoe UI Semilight" }, { file: "bahnschrift.ttf", label: "Bahnschrift" }];
  const programs = [
    { exe: "afterfx.exe", title: "Adobe After Effects 2026 - proyecto_final_version_definitiva_de_verdad_esta_si_que_es_la_ultima.aep" },
    { exe: "blender.exe", title: "Blender 4.5" },
    { exe: "wallpaper64.exe", title: "Wallpaper Engine" },
    { exe: "obs64.exe", title: "OBS 31.0.2" },
    { exe: "zoom.exe", title: "Zoom Workplace" },
  ];
  const json = (body) => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
  const real = window.fetch.bind(window);
  window.fetch = async (url, options) => {
    const path = String(url);
    if (!path.includes("/api/")) return real(url, options);
    if (path.includes("/api/state")) return json(state);
    if (path.includes("/api/themes/hidden")) return json(params.has("hidden") ? [theme("pulse", "Mars", true), theme("signal", "Reef", true), theme("arcade", "Block Island", true)] : []);
    if (path.includes("/api/themes")) return json(list);
    if (path.includes("/api/theme?")) return real("/shots-theme.json");
    if (path.includes("/api/fonts")) return json(fonts);
    if (path.includes("/api/places")) return json([{ name: "Bogotá", admin1: "Bogotá D.C.", country: "Colombia", latitude: 4.6, longitude: -74.08 }]);
    if (path.includes("/api/programs")) return json(programs);
    if (path.includes("/api/update")) return json({ version: params.get("update") });
    if (path.includes("/api/preview")) return real("/shots/pulse-1x.png");
    return json({ ok: true });
  };
  window.__page = params.get("page");
  // abre un diálogo en las capturas: pulsa el primer botón con ese texto
  const click = params.get("click");
  if (click) setTimeout(() => [...document.querySelectorAll("button")].find((b) => b.textContent.includes(click))?.click(), 1500);
  // las imágenes de /api/ van a los PNG de ejemplo
  new MutationObserver(() => {
    document.querySelectorAll("img").forEach((img) => {
      const src = img.getAttribute("src") ?? "";
      if (!src.includes("/api/")) return;
      const id = /id=([a-z]+)/.exec(src)?.[1];
      img.src = id && id !== "tierra" ? `/shots/${id}-1x.png` : "/shots/pulse-1x.png";
    });
  }).observe(document, { subtree: true, childList: true, attributes: true, attributeFilter: ["src"] });
})();
