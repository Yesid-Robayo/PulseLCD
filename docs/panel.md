# The panel · El panel

[English](#english) · [Español](#español)

## English

What follows was measured on a real Turing Smart Screen 3.5" (`1A86:5722`), not assumed. It explains why `crates/panel` is written the
way it is.

### What the panel doesn't do

- **It never answers.** Not even to the hello command: 0 bytes, always, healthy or not. There is no way to ask how it is.
- **A command has no marker.** It's six bytes, and the panel tells commands from pixels only by counting.

### Numbers

| Measure | Value |
| --- | --- |
| Full frame | 307,200 bytes (480×320, 2 bytes per pixel) |
| Time for a full frame while drawing | 2,400 to 2,401 ms, always |
| Right after a resync | 2,406 to 2,407 ms |
| The same bytes without drawing | 2,328 ms |
| USB gone after a reset | About 2 s |

The gap between 2,400 and 2,328 ms never overlapped, so `MIN_FRAME_MS` sits in the middle, at 2,365 ms. That time is the only health
signal the panel gives.

Frames far below both times have also been seen, around 2,000 ms, with no USB drop, and nobody knows why yet: CPU load alone doesn't
produce them. PulseLCD resynchronises anyway, which does no harm, but it only counts a repair if the repeated frame comes back with the
normal time.

### An image cut in half

If a session ends in the middle of an image, the panel stays waiting for pixels. When the next session starts, its commands are
swallowed as pixels, and part of its image is then read as commands. Sooner or later some pixels look like a reset:

| Test | Result |
| --- | --- |
| Cut + normal start, 4 alignments | The panel reset itself and dropped off USB, 4 of 4 |
| Cut + resync + normal start | Clean picture and no USB drop, 12 of 12 in the final code |

The resync is two steps: zeros until any image in progress is finished, then a train of 1×1 images at the corner. A healthy panel draws
them and only changes one pixel, which the next frame restores. A misaligned one falls into step within a few repeats, because 6 and 8
bytes cross each other. A model of the parser as a stream of 6-byte groups did not predict that odd alignments recover too, so the real
parser probably restarts on each USB packet.

### What it isn't

- **Screen off isn't the cause.** Turning the screen off and running the normal start is clean.
- **Reset isn't needed to resync.** And it isn't free: twice, a reset on a panel already in trouble made it drop off USB four times in
  10 seconds, and it didn't come back until it was unplugged. So PulseLCD never resets on its own.

### Still unknown

- **Blue stripes with no USB drop:** seen once after a hybrid boot, with widgets fine and the date sheared inside its box. It couldn't be
  reproduced. A single later repaint cleaned it at once, which is why repaints exist.
- **Black screen with the backlight on:** the LCD hangs. Only cutting the power recovers it. On the test machine 21 of 63 hybrid boots
  failed against 0 of 21 full boots, but most full boots were restarts, so that's a hint, not proof.

## Español

Lo que sigue se midió en una Turing Smart Screen 3,5" real (`1A86:5722`), no se supuso. Explica por qué `crates/panel` está escrito como
está.

### Lo que el panel no hace

- **Nunca responde.** Ni siquiera al comando de saludo: 0 bytes, siempre, esté sano o no. No hay forma de preguntarle cómo está.
- **Un comando no lleva marca.** Son seis bytes, y el panel distingue comandos de píxeles solo contando.

### Cifras

| Medida | Valor |
| --- | --- |
| Cuadro completo | 307.200 bytes (480×320, 2 bytes por píxel) |
| Tiempo de un cuadro completo dibujando | 2.400 a 2.401 ms, siempre |
| Justo tras resincronizar | 2.406 a 2.407 ms |
| Los mismos bytes sin dibujar | 2.328 ms |
| USB ausente tras un reinicio | Unos 2 s |

La diferencia entre 2.400 y 2.328 ms nunca se solapó, así que `MIN_FRAME_MS` queda en medio, en 2.365 ms. Ese tiempo es la única señal de
salud que da el panel.

También se han visto cuadros muy por debajo de los dos tiempos, de unos 2.000 ms, sin caída del USB, y aún no se sabe por qué: la carga de
CPU por sí sola no los produce. PulseLCD resincroniza igualmente, que no hace daño, pero solo cuenta una reparación si el cuadro repetido
vuelve con el tiempo normal.

### Una imagen cortada por la mitad

Si una sesión termina a mitad de una imagen, el panel se queda esperando píxeles. Al empezar la siguiente, sus comandos se los traga como
píxeles, y parte de su imagen se lee después como comandos. Tarde o temprano unos píxeles parecen un reinicio:

| Prueba | Resultado |
| --- | --- |
| Corte + arranque normal, 4 alineaciones | El panel se reinició solo y cayó del USB, 4 de 4 |
| Corte + resincronización + arranque normal | Imagen limpia y sin caída del USB, 12 de 12 en el código final |

La resincronización son dos pasos: ceros hasta terminar cualquier imagen en curso, y luego un tren de imágenes de 1×1 en la esquina. Un
panel sano las dibuja y solo cambia un píxel, que el cuadro siguiente restaura. Uno desalineado entra en fase en pocas repeticiones, porque
6 y 8 bytes se cruzan. Un modelo del parser como flujo de grupos de 6 bytes no predijo que las alineaciones impares también se recuperan,
así que el parser real probablemente reinicia con cada paquete USB.

### Lo que no es

- **Apagar la pantalla no es la causa.** Apagarla y hacer el arranque normal sale limpio.
- **Reiniciar no hace falta para resincronizar.** Y no sale gratis: dos veces, un reinicio sobre un panel ya en problemas lo hizo caer del
  USB cuatro veces en 10 segundos, y no volvió hasta desconectarlo. Por eso PulseLCD nunca reinicia por su cuenta.

### Lo que sigue sin saberse

- **Franjas azules sin caída del USB:** se vio una vez tras un arranque híbrido, con los widgets bien y la fecha cizallada dentro de su caja.
  No se pudo reproducir. Un solo repintado posterior lo limpió al instante, y por eso existen los repintados.
- **Pantalla negra con la luz encendida:** se cuelga la LCD. Solo cortar la energía la recupera. En el equipo de pruebas fallaron 21 de 63
  arranques híbridos frente a 0 de 21 completos, pero casi todos los completos fueron reinicios, así que es un indicio, no una prueba.
