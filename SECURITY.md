# Security · Seguridad

[English](#english) · [Español](#español)

## English

### How to report a security problem

If you find a security flaw (for example, a way to make PulseLCD run something, to write files outside its folder or to use the
sensors helper to get administrator rights), **do not post it in an *issue***. Use GitHub's private report:

1. The repository's **Security** tab.
2. **Report a vulnerability**.
3. Describe what happens, how to reproduce it and with which version.

Only the author will see it. You will get an answer as soon as possible and, once it is fixed, a new version will be released.

### Supported versions

Only the latest version published in *Releases* receives fixes.

### How PulseLCD is built

- **It runs without administrator rights.** The tray agent and the window are normal processes.
- **Only the sensors helper is elevated,** and only if you enable it. `PulseLCD-Sensors.exe` reads the CPU temperature and FPS. It
  doesn't open themes, images or fonts, doesn't touch the network, writes no files, not even a log, and the only thing it accepts
  through its pipe is a one-byte order. Installing it needs your permission once: it copies the helper to
  `C:\Program Files\PulseLCD`, which only an administrator can change, and creates one scheduled task, `PulseLCD-Sensors`, pointing
  there. So no program running as your user can replace what runs with those rights.
- **Only CPU temperature reads hardware,** through PawnIO, a signed driver that only runs signed modules. PulseLCD doesn't install it
  and never writes to hardware. FPS is measured with the tool in your AMD or NVIDIA driver or, if there's none, the included
  PresentMon. It runs only while the program in front isn't on the ignored list,
  even if it isn't a game, and stops by itself a minute later.
- **Themes are treated as untrusted,** because they can be shared. Their values are checked before use and they can't reach outside
  their folder.
- **The window can't run anything.** It loads only pages built into the exe, can't navigate elsewhere, and its bridge to Rust has a
  fixed list of actions with no free paths.
- **It's careful with the panel.** It only sends commands the panel is known to understand, and never tries unknown ones.

### What leaves your PC

- **Weather:** if you turn it on, the coordinates you chose go to Open-Meteo.
- **Latency:** if your theme shows it, a ping goes to the address in Settings (`1.1.1.1` by default).
- **New versions:** if you turn the notice on, the window asks GitHub for the latest version number each time it opens. It's off by
  default, and nothing is downloaded or installed.
- **Nothing else.** There's no telemetry and no accounts. The battery source can only be a local address.

### Your data

Everything is in `%APPDATA%\PulseLCD`: settings, your themes and the log, which doesn't record paths with your user name or your
location. The window's browser data is in `%LOCALAPPDATA%\PulseLCD`.

### What it changes in your system

| What | Where | When |
| --- | --- | --- |
| Start with Windows | `HKEY_CURRENT_USER\...\Run`, value `PulseLCD` | If you turn it on |
| Sensors helper | Scheduled task `PulseLCD-Sensors` and folder `C:\Program Files\PulseLCD` | If you enable it |

To remove everything: turn off **Start with Windows**, press **Remove** next to the helper in Settings, and delete the program
folder, `%APPDATA%\PulseLCD` and `%LOCALAPPDATA%\PulseLCD`.

### Verifying a download

Each release has its `SHA256SUMS.txt` and a build provenance attestation, which proves the files came from this code and GitHub's
workflow:

```powershell
gh attestation verify PulseLCD-0.1.1.zip --repo Yesid-Robayo/PulseLCD
```

## Español

### Cómo avisar de un problema de seguridad

Si encuentras un fallo de seguridad (por ejemplo, una forma de hacer que PulseLCD ejecute algo, de escribir archivos fuera de su carpeta
o de usar el ayudante de sensores para conseguir permisos de administrador), **no lo publiques en un *issue***. Usa el aviso privado de
GitHub:

1. Pestaña **Security** del repositorio.
2. **Report a vulnerability**.
3. Describe qué pasa, cómo repetirlo y con qué versión.

Solo lo verá el autor. Recibirás respuesta lo antes posible y, cuando se corrija, saldrá una versión nueva.

### Versiones con soporte

Solo la última versión publicada en *Releases* recibe correcciones.

### Cómo está hecho PulseLCD

- **Funciona sin permisos de administrador.** El agente de la bandeja y la ventana son procesos normales.
- **Solo el ayudante de sensores corre elevado,** y solo si lo activas. `PulseLCD-Sensors.exe` lee la temperatura del CPU y los FPS. No
  abre temas, imágenes ni fuentes, no toca la red, no escribe archivos, ni siquiera un log, y lo único que acepta por su tubería es una
  orden de un byte. Instalarlo necesita tu permiso una vez: copia el ayudante a `C:\Program Files\PulseLCD`, que solo un administrador
  puede cambiar, y crea una tarea programada, `PulseLCD-Sensors`, que apunta ahí. Así ningún programa que corra con tu usuario puede
  reemplazar lo que se ejecuta con esos permisos.
- **Solo la temperatura del CPU lee hardware,** con PawnIO, un driver firmado que solo ejecuta módulos firmados. PulseLCD no lo instala
  y nunca escribe en el hardware. Los FPS se miden con la herramienta de tu driver de AMD o NVIDIA o, si no hay, con el PresentMon
  incluido. Solo corre mientras el programa en primer plano no esté en la lista de ignorados,
  aunque no sea un juego, y se detiene sola un minuto después.
- **Los temas se tratan como no confiables,** porque se pueden compartir. Sus valores se comprueban antes de usarlos y no pueden salirse
  de su carpeta.
- **La ventana no puede ejecutar nada.** Solo carga páginas incluidas en el exe, no puede navegar a otro sitio, y su puente con Rust
  tiene una lista fija de acciones, sin rutas libres.
- **Es cuidadoso con el panel.** Solo envía comandos que se sabe que el panel entiende, y nunca prueba otros.

### Qué sale de tu PC

- **Clima:** si lo activas, las coordenadas que elegiste van a Open-Meteo.
- **Latencia:** si tu tema la muestra, sale un ping a la dirección de Ajustes (`1.1.1.1` por defecto).
- **Versiones nuevas:** si activas el aviso, la ventana pregunta a GitHub el número de la última versión cada vez que se abre. Viene
  apagado, y no se descarga ni se instala nada.
- **Nada más.** No hay telemetría ni cuentas. La fuente de baterías solo puede ser una dirección local.

### Tus datos

Todo está en `%APPDATA%\PulseLCD`: los ajustes, tus temas y el registro, que no guarda rutas con tu nombre de usuario ni tu ubicación.
Los datos del navegador de la ventana están en `%LOCALAPPDATA%\PulseLCD`.

### Qué cambia en tu sistema

| Qué | Dónde | Cuándo |
| --- | --- | --- |
| Iniciar con Windows | `HKEY_CURRENT_USER\...\Run`, valor `PulseLCD` | Si lo activas |
| Ayudante de sensores | Tarea programada `PulseLCD-Sensors` y carpeta `C:\Program Files\PulseLCD` | Si lo activas |

Para quitarlo todo: apaga **Iniciar con Windows**, pulsa **Quitar** junto al ayudante en Ajustes y borra la carpeta del programa,
`%APPDATA%\PulseLCD` y `%LOCALAPPDATA%\PulseLCD`.

### Verificar una descarga

Cada versión trae su `SHA256SUMS.txt` y una atestación de origen, que demuestra que los archivos salieron de este código y del workflow
de GitHub:

```powershell
gh attestation verify PulseLCD-0.1.1.zip --repo Yesid-Robayo/PulseLCD
```
