# Architecture · Arquitectura

[English](#english) · [Español](#español)

## English

### Overview

| Crate | Contents |
| --- | --- |
| `crates/shared` | What every process shares: metrics, configuration, theme format, messages. |
| `crates/panel` | The panel: protocol, serial port, what to send, and the guardian. |
| `crates/render` | From data to image. No device, no state about what changed. |
| `crates/sensors` | Metrics that need no privileges. |
| `app` (`PulseLCD.exe`) | The agent, the tray and the window. |
| `broker` (`PulseLCD-Sensors.exe`) | The only elevated process. |

### Three processes

| Process | Privileges | Lives | Job |
| --- | --- | --- | --- |
| Agent (`--agent`) | Normal | Always, in the tray | Owns the panel, the configuration and the sensors. |
| Helper | Administrator | Always, if enabled | CPU temperature and FPS, nothing else. |
| Window (no arguments) | Normal | While it's open | The interface. Closing it frees everything. |

The window talks to the agent through one named pipe: length-prefixed messages, JSON for orders and status, raw RGB565 for the mirror.
Nothing is written to disk while the window is closed. The agent is the only writer of `config.json`.

The helper's protocol is deliberately poor: one byte in, one fixed-size sample out. There's no text to interpret with privileges, and it
reads no files, so a theme or a font can never reach elevated code.

### The cycle

`app/src/agent/pipeline.rs` is a single thread that receives everything as orders in a queue, so nothing else can touch the port:

1. Read only the metrics that something will show.
2. `Scene::paint`: a pure function from data to image. The panel, the mirror and the editor all use it.
3. `Guardian::show`: compares with what's on the panel and sends the difference.

### The guardian

`crates/panel/src/guardian.rs` follows four rules, explained in [panel.md](panel.md): resync at the start of every session, never reset on
its own, repaint from time to time, and check how long full frames take. It only knows the `Bus`, `Wire` and `Clock` traits, so its tests
run against `simulator.rs`, a fake panel that reads pixels as commands just like the real one, consumes bytes at the measured speed and
fails on demand.

### The window

The interface is HTML, CSS and JavaScript with no framework or build step, embedded in the exe and shown with WebView2 through `wry`. It's
served by a custom protocol, so no other program can reach it, and `app/src/window/api.rs` is a fixed list of routes. The editor posts the
theme on each change and gets back the image drawn by `pulse-render`, so the preview can't differ from the panel.

`app/web/dev/preview.html` runs the same interface with fake data, to review it in a normal browser. It isn't included in the exe.

## Español

### Visión general

| Crate | Contenido |
| --- | --- |
| `crates/shared` | Lo que comparten todos los procesos: métricas, configuración, formato de temas, mensajes. |
| `crates/panel` | El panel: protocolo, puerto serie, qué enviar y el guardián. |
| `crates/render` | Del dato a la imagen. Sin dispositivos y sin estado de qué cambió. |
| `crates/sensors` | Métricas que no necesitan privilegios. |
| `app` (`PulseLCD.exe`) | El agente, la bandeja y la ventana. |
| `broker` (`PulseLCD-Sensors.exe`) | El único proceso elevado. |

### Tres procesos

| Proceso | Privilegios | Vive | Trabajo |
| --- | --- | --- | --- |
| Agente (`--agent`) | Normales | Siempre, en la bandeja | Dueño del panel, la configuración y los sensores. |
| Ayudante | Administrador | Siempre, si se activa | Temperatura del CPU y FPS, nada más. |
| Ventana (sin argumentos) | Normales | Mientras está abierta | La interfaz. Al cerrarla se libera todo. |

La ventana habla con el agente por una tubería con nombre: mensajes con su longitud delante, JSON para órdenes y estado, y RGB565 directo para
el espejo. Con la ventana cerrada no se escribe nada en disco. El agente es el único que escribe `config.json`.

El protocolo del ayudante es pobre a propósito: un byte de entrada y una lectura de tamaño fijo de salida. No hay texto que interpretar con
privilegios, y no lee archivos, así que un tema o una fuente nunca pueden llegar a código elevado.

### El ciclo

`app/src/agent/pipeline.rs` es un solo hilo que lo recibe todo como órdenes en una cola, así que nada más puede tocar el puerto:

1. Lee solo las métricas que algo va a mostrar.
2. `Scene::paint`: una función pura de datos a imagen. La usan el panel, el espejo y el editor.
3. `Guardian::show`: compara con lo que hay en el panel y envía la diferencia.

### El guardián

`crates/panel/src/guardian.rs` sigue cuatro reglas, explicadas en [panel.md](panel.md): resincronizar al empezar cada sesión, no reiniciar nunca
por su cuenta, repintar de vez en cuando y comprobar cuánto tardan los cuadros completos. Solo conoce las interfaces `Bus`, `Wire` y `Clock`, así
que sus pruebas corren contra `simulator.rs`, un panel de mentira que lee píxeles como comandos igual que el real, consume bytes a la velocidad
medida y falla a voluntad.

### La ventana

La interfaz es HTML, CSS y JavaScript sin framework ni compilación, incluida en el exe y mostrada con WebView2 a través de `wry`. Se sirve por un
protocolo propio, así que ningún otro programa puede llegar a ella, y `app/src/window/api.rs` es una lista fija de rutas. El editor envía el tema
en cada cambio y recibe la imagen dibujada por `pulse-render`, así que la vista previa no puede diferir del panel.

`app/web/dev/preview.html` corre la misma interfaz con datos inventados, para revisarla en un navegador normal. No va incluido en el exe.
