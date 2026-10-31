"""Genera los fondos de los temas incluidos en themes/images.

Todo sale de este archivo: ruido, geometría y óptica, sin fotos ni imágenes de terceros. Por eso los fondos se distribuyen con
la misma licencia que el resto del repositorio. Cada escena usa semillas fijas, así que el resultado siempre es el mismo.

Uso: python scripts/make_backgrounds.py [escena ...]   Necesita numpy y Pillow.
"""
import argparse
import pathlib

import numpy as np
from PIL import Image, ImageDraw

WIDTH, HEIGHT = 480, 320
SCALE = 3
QUALITY = 94
OUT = pathlib.Path(__file__).resolve().parent.parent / "themes" / "images"


class Noise:
    """Ruido de gradientes en 3D, sobre una tabla fija que depende de la semilla."""

    SIZE = 4096

    def __init__(self, seed):
        rng = np.random.default_rng(seed)
        self.table = rng.permutation(self.SIZE)
        gradients = rng.normal(size=(self.SIZE, 3))
        self.gradients = (gradients / np.linalg.norm(gradients, axis=1, keepdims=True)).astype(np.float32)

    def __call__(self, x, y, z):
        cell = [np.floor(v).astype(np.int32) for v in (x, y, z)]
        part = [(v - c).astype(np.float32) for v, c in zip((x, y, z), cell)]
        mask = self.SIZE - 1

        def corner(dx, dy, dz):
            hashed = self.table[(self.table[(self.table[(cell[0] + dx) & mask] + cell[1] + dy) & mask] + cell[2] + dz) & mask]
            g = self.gradients[hashed]
            return g[..., 0] * (part[0] - dx) + g[..., 1] * (part[1] - dy) + g[..., 2] * (part[2] - dz)

        fade = [t * t * t * (t * (t * 6 - 15) + 10) for t in part]
        low = (corner(0, 0, 0) * (1 - fade[0]) + corner(1, 0, 0) * fade[0]) * (1 - fade[1]) \
            + (corner(0, 1, 0) * (1 - fade[0]) + corner(1, 1, 0) * fade[0]) * fade[1]
        high = (corner(0, 0, 1) * (1 - fade[0]) + corner(1, 0, 1) * fade[0]) * (1 - fade[1]) \
            + (corner(0, 1, 1) * (1 - fade[0]) + corner(1, 1, 1) * fade[0]) * fade[1]
        return (low * (1 - fade[2]) + high * fade[2]) * 1.5


def fbm(noise, x, y, z, octaves=6, gain=0.5):
    total, amplitude, weight = 0.0, 1.0, 0.0
    for _ in range(octaves):
        total = total + amplitude * noise(x, y, z)
        weight += amplitude
        amplitude *= gain
        x, y, z = x * 2.03 + 13.1, y * 2.03 - 7.7, z * 2.03 + 3.3
    return total / weight


def ridged(noise, x, y, z, octaves=5, gain=0.5):
    total, amplitude, weight = 0.0, 1.0, 0.0
    for _ in range(octaves):
        total = total + amplitude * (1 - np.abs(noise(x, y, z)) * 2)
        weight += amplitude
        amplitude *= gain
        x, y, z = x * 2.03 + 5.2, y * 2.03 + 1.3, z * 2.03 - 9.1
    return total / weight


def smooth(low, high, x):
    t = np.clip((x - low) / (high - low), 0, 1)
    return t * t * (3 - 2 * t)


def color(*rgb):
    return np.array(rgb, dtype=np.float32)


def mix(a, b, t):
    t = np.asarray(t)[..., None]
    return a * (1 - t) + b * t


def normalize(*v):
    v = np.array(v, dtype=np.float32)
    return v / np.linalg.norm(v)


def blur(image, sigma):
    """Desenfoque gaussiano por FFT: el coste no depende de sigma."""
    h, w = image.shape[:2]
    fy, fx = np.fft.fftfreq(h)[:, None], np.fft.rfftfreq(w)[None, :]
    kernel = np.exp(-2 * (np.pi * sigma) ** 2 * (fx * fx + fy * fy))
    if image.ndim == 2:
        return np.fft.irfft2(np.fft.rfft2(image) * kernel, s=(h, w)).astype(np.float32)
    return np.stack([blur(image[..., i], sigma) for i in range(image.shape[2])], axis=-1)


def grid(scale=SCALE):
    h, w = HEIGHT * scale, WIDTH * scale
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    return (x + 0.5) / scale, (y + 0.5) / scale


def stars(seed, count, scale=SCALE, brightness=1.0):
    """Estrellas con brillo en ley de potencia y algo de color, ya con su desenfoque."""
    rng = np.random.default_rng(seed)
    h, w = HEIGHT * scale, WIDTH * scale
    layer = np.zeros((h, w, 3), dtype=np.float32)
    energy = (rng.pareto(2.1, count) + 0.35) * 0.13 * brightness * scale * scale
    tint = mix(color(1.0, 0.82, 0.68), color(0.72, 0.84, 1.0), rng.random(count))
    layer[rng.integers(0, h, count), rng.integers(0, w, count)] = (tint * np.minimum(energy, 7 * scale * scale)[:, None])
    return blur(layer, 0.34 * scale)


def bloom(image, threshold, sigma, strength):
    bright = np.maximum(image - threshold, 0)
    return image + blur(bright, sigma) * strength


def finish(name, linear, scale=SCALE, exposure=1.0):
    """Reduce en luz lineal, comprime los brillos y añade un poco de grano para que no se vean escalones."""
    h, w = linear.shape[:2]
    small = linear.reshape(h // scale, scale, w // scale, scale, 3).mean(axis=(1, 3))
    toned = 1 - np.exp(-np.maximum(small, 0) * exposure)
    srgb = np.power(toned, 1 / 2.2)
    rng = np.random.default_rng(1)
    srgb = srgb + rng.normal(0, 0.0035, srgb.shape)
    picture = Image.fromarray((np.clip(srgb, 0, 1) * 255 + 0.5).astype(np.uint8))
    OUT.mkdir(parents=True, exist_ok=True)
    picture.save(OUT / f"{name}.jpg", quality=QUALITY, subsampling=0, optimize=True)
    print(name, (OUT / f"{name}.jpg").stat().st_size // 1024, "KB")


def rotate(x, y, z, tilt, spin):
    """Inclina el eje y gira el planeta, para no ver siempre la misma cara."""
    c, s = np.cos(tilt), np.sin(tilt)
    x, y = x * c - y * s, x * s + y * c
    c, s = np.cos(spin), np.sin(spin)
    return x * c + z * s, y, z * c - x * s


def disc(x, y, cx, cy, radius):
    u, v = (x - cx) / radius, (y - cy) / radius
    rr = u * u + v * v
    return u, v, np.sqrt(np.maximum(1 - rr, 0)), rr


def rim_glow(u, v, rr, light, tint, width, power=1.0):
    """Atmósfera por fuera del disco: más fuerte hacia el sol y casi nula en la cara de noche."""
    r = np.sqrt(rr)
    facing = smooth(-0.55, 0.65, (u * light[0] + v * light[1]) / np.maximum(r, 1e-3))
    glow = np.exp(-np.maximum(r - 1, 0) / width) * facing * power
    return (glow * (rr >= 1))[..., None] * tint


def earth(x, y, cx, cy, radius, light, seed, tilt=0.4, spin=1.3, detail=7, sun=1.0):
    """Un planeta como la Tierra, con continentes inventados. Devuelve su luz y cuánto cubre cada píxel."""
    u, v, w, rr = disc(x, y, cx, cy, radius)
    inside = rr < 1
    image = np.zeros(x.shape + (3,), dtype=np.float32)
    nx, ny, nz = u[inside], v[inside], w[inside]
    px, py, pz = rotate(nx, ny, nz, tilt, spin)
    a, b, c = Noise(seed), Noise(seed + 1), Noise(seed + 2)

    wx, wy = fbm(b, px * 1.6, py * 1.6, pz * 1.6, 4), fbm(c, px * 1.6 + 4, py * 1.6, pz * 1.6, 4)
    coast_x, coast_y = fbm(c, px * 6, py * 6, pz * 6 + 2, 4), fbm(b, px * 6 + 3, py * 6, pz * 6, 4)
    height = fbm(a, px * 1.25 + wx * 0.7 + coast_x * 0.09, py * 1.25 + wy * 0.7 + coast_y * 0.09, pz * 1.25 - wx * 0.5, detail, 0.55)
    latitude = np.abs(py)
    land = smooth(0.0, 0.008, height - 0.075)

    moisture = fbm(b, px * 2.4 + 9, py * 2.4, pz * 2.4, 5) + 0.45 * np.cos(latitude * 7.5)
    ground = mix(color(0.2, 0.135, 0.07), color(0.022, 0.06, 0.02), smooth(-0.05, 0.3, moisture))
    ground = mix(ground, color(0.11, 0.1, 0.09), smooth(0.14, 0.34, height))
    ground = mix(ground, color(0.72, 0.77, 0.82), smooth(0.8, 0.9, latitude + 0.25 * height + 0.05 * wx))
    shallow = smooth(-0.09, 0.0, height - 0.075)
    ocean = mix(color(0.0022, 0.0095, 0.04), color(0.006, 0.05, 0.1), shallow * shallow)
    surface = mix(ocean, ground, land)

    swirl = fbm(c, px * 2.1, py * 3.1, pz * 2.1, 3)
    cloud = fbm(a, px * 3.0 + swirl * 1.8 + 5, py * 5.2 + swirl, pz * 3.0 - swirl, detail, 0.6)
    cloud = smooth(0.07, 0.3, cloud + 0.05 * np.cos(latitude * 9)) ** 1.3
    surface = surface * (1 - 0.5 * cloud[..., None])
    surface = mix(surface, color(0.72, 0.75, 0.8), cloud * 0.9)

    ndotl = nx * light[0] + ny * light[1] + nz * light[2]
    day = smooth(-0.02, 0.5, ndotl) * (0.25 + 0.75 * np.clip(ndotl, 0, 1))
    half = normalize(light[0], light[1], light[2] + 1)
    glint = np.clip(nx * half[0] + ny * half[1] + nz * half[2], 0, 1) ** 260 * (1 - land) * (1 - cloud) * day
    lit = surface * (day * 2.1 * sun)[..., None] + (glint * 1.4 * sun)[..., None] * color(1.0, 0.95, 0.85)

    # luces de las ciudades: cúmulos cerca de las costas, y solo donde ya es de noche
    night = smooth(0.1, -0.12, ndotl)
    people = smooth(0.0, 0.35, fbm(b, px * 3.4 - 3, py * 3.4, pz * 3.4, 3) + 0.34 * smooth(0.035, 0.0, height - 0.075))
    points = smooth(0.62, 0.8, ridged(c, px * 74, py * 74, pz * 74, 3)) * smooth(0.04, 0.3, fbm(a, px * 26, py * 26, pz * 26, 3))
    cities = land * people * points * night * (1 - 0.85 * cloud) * smooth(0.9, 0.55, latitude)
    lit = lit + (cities * 4.6 * sun)[..., None] * color(1.0, 0.6, 0.24)

    limb = np.exp(-nz * 3.4)
    sunlit = smooth(-0.3, 0.45, ndotl)
    lit = lit + (limb * sunlit * 1.0 * sun)[..., None] * color(0.1, 0.36, 1.0)
    twilight = np.exp(-(ndotl / 0.1) ** 2) * cloud
    lit = lit + (twilight * 0.32)[..., None] * color(1.0, 0.42, 0.16)

    image[inside] = lit
    edge = np.clip((1 - np.sqrt(rr)) * radius * SCALE * 0.5, 0, 1)
    glow = rim_glow(u, v, rr, light, color(0.16, 0.45, 1.0), 0.03, 1.3)
    return image + glow, edge


def scene_orbit():
    x, y = grid()
    cx, cy, radius = 338, 164, 150
    light = normalize(-0.93, 0.08, 0.36)
    planet, cover = earth(x, y, cx, cy, radius, light, 11)
    haze = Noise(70)
    sky = smooth(-0.3, 0.5, fbm(haze, x / 110, y / 110, 3, 5))[..., None] * color(0.002, 0.009, 0.034)
    sky = sky * (1 - smooth(150, 330, x))[..., None] + stars(71, 300)
    return bloom(sky * (1 - cover[..., None]) + planet, 0.9, 2.5 * SCALE, 0.3)


def craters(px, py, pz, seed, count, largest, smallest):
    """Cráteres sobre una esfera: fondo hundido y borde levantado."""
    rng = np.random.default_rng(seed)
    centers = rng.normal(size=(count, 3))
    centers /= np.linalg.norm(centers, axis=1, keepdims=True)
    sizes = smallest * (largest / smallest) ** (rng.random(count) ** 3.2)
    relief = np.zeros(px.shape, dtype=np.float32)
    for center, size in zip(centers.astype(np.float32), sizes):
        near = (px * center[0] + py * center[1] + pz * center[2]) > 1 - 2.5 * size * size
        if not near.any():
            continue
        distance = np.sqrt(np.maximum(2 - 2 * (px[near] * center[0] + py[near] * center[1] + pz[near] * center[2]), 0)) / size
        bowl = -(1 - distance * distance) * (distance < 1)
        wall = np.exp(-((distance - 1) / 0.17) ** 2) * 0.55 * smooth(1.55, 1.0, distance)
        relief[near] += (bowl + wall) * size
    return relief


def bumped(height, nx, ny, nz, inside, strength):
    """Inclina la normal según el relieve, medido en pantalla."""
    field = np.zeros(inside.shape, dtype=np.float32)
    field[inside] = height
    field = blur(field, 0.55)
    dy, dx = np.gradient(field)
    bx, by = nx - dx[inside] * strength, ny - dy[inside] * strength
    length = np.sqrt(bx * bx + by * by + nz * nz)
    return bx / length, by / length, nz / length


def scene_mars():
    x, y = grid()
    cx, cy, radius = 150, 172, 168
    light = normalize(-0.5, -0.45, 0.74)
    u, v, w, rr = disc(x, y, cx, cy, radius)
    inside = rr < 1
    nx, ny, nz = u[inside], v[inside], w[inside]
    px, py, pz = rotate(nx, ny, nz, 0.42, 0.9)
    a, b = Noise(21), Noise(22)
    warp = fbm(b, px * 1.8, py * 1.8, pz * 1.8, 4)
    regions = fbm(a, px * 1.5 + warp, py * 1.5, pz * 1.5 - warp, 7)
    fine = fbm(b, px * 11, py * 11, pz * 11, 5)
    height = regions * 0.07 + fine * 0.01 + craters(px, py, pz, 23, 340, 0.1, 0.005) * 0.3
    valley = smooth(0.1, 0.0, np.abs(fbm(a, px * 0.7 + 3, py * 3.2, pz * 0.7, 4) - 0.02)) * smooth(0.6, 0.3, np.abs(py))
    height = height - valley * 0.008
    bx, by, bz = bumped(height, nx, ny, nz, inside, 0.8 * radius * SCALE)

    surface = mix(color(0.4, 0.105, 0.04), color(0.1, 0.038, 0.025), smooth(-0.1, 0.22, regions + fine * 0.3))
    surface = mix(surface, color(0.58, 0.24, 0.1), smooth(0.05, 0.3, -regions + 0.4 * fine) * 0.55)
    surface = surface * (1 - 0.35 * valley[..., None])
    ice = smooth(0.87, 0.93, py * -1 + 0.05 * fine * 3 + 0.03 * warp) + smooth(0.92, 0.97, py + 0.1 * fine)
    surface = mix(surface, color(0.92, 0.88, 0.84), np.clip(ice, 0, 1))

    ndotl = bx * light[0] + by * light[1] + bz * light[2]
    smooth_side = nx * light[0] + ny * light[1] + nz * light[2]
    day = np.clip(ndotl, 0, 1) * smooth(-0.05, 0.25, smooth_side)
    lit = surface * (day * 2.0)[..., None]
    limb = np.exp(-nz * 6) * smooth(-0.15, 0.4, smooth_side)
    lit = lit + (limb * 0.42)[..., None] * color(1.0, 0.52, 0.32)

    image = stars(24, 260)
    image = image * (1 - np.clip((1 - np.sqrt(rr)) * radius * SCALE * 0.5, 0, 1))[..., None]
    image[inside] += lit
    image = image + rim_glow(u, v, rr, light, color(0.95, 0.45, 0.26), 0.014, 0.32)
    return image


RING_INNER, RING_OUTER = 1.25, 2.3


def ring_density(r, noise):
    """Las bandas de los anillos, con sus divisiones."""
    t = (r - RING_INNER) / (RING_OUTER - RING_INNER)
    fine = fbm(noise, r * 19, r * 0.1, r * 0.1, 5) * 0.62 + 0.72
    density = smooth(0.0, 0.03, t) * smooth(1.0, 0.985, t) * fine
    density = density * mix_scalar(0.28, 0.92, smooth(0.2, 0.3, t))
    density = density * (1 - smooth(0.66, 0.665, t) * smooth(0.735, 0.73, t))
    density = density * (1 - 0.9 * smooth(0.905, 0.908, t) * smooth(0.917, 0.914, t))
    return density * ((r > RING_INNER) & (r < RING_OUTER))


def mix_scalar(a, b, t):
    return a * (1 - t) + b * t


def scene_saturn():
    x, y = grid()
    cx, cy, radius = 288, 128, 86
    angle, opening = np.radians(-17), np.radians(21)
    axis = np.array([np.cos(angle), np.sin(angle), 0], dtype=np.float32)
    side = np.array([-np.sin(angle), np.cos(angle), 0], dtype=np.float32)
    normal = side * np.cos(opening) - np.array([0, 0, 1], dtype=np.float32) * np.sin(opening)
    light = normalize(-0.72, -0.28, 0.63)
    noise = Noise(31)

    px, py = (x - cx) / radius, (y - cy) / radius
    along = px * side[0] + py * side[1]
    across = px * axis[0] + py * axis[1]
    flat = 0.92
    squashed = along / flat
    rr = across * across + squashed * squashed
    inside = rr < 1
    pz = np.sqrt(np.maximum(1 - rr, 0))

    image = stars(32, 240)
    # anillos: el punto del plano por el que pasa cada rayo de la cámara
    ring_z = -(px * normal[0] + py * normal[1]) / normal[2]
    ring_r = np.sqrt(px * px + py * py + ring_z * ring_z)
    density = ring_density(ring_r, noise)
    t = np.clip((ring_r - RING_INNER) / (RING_OUTER - RING_INNER), 0, 1)
    ring_color = mix(color(0.3, 0.24, 0.18), color(0.62, 0.5, 0.36), smooth(0.1, 0.5, t))
    # sombra del planeta sobre los anillos
    along_light = px * light[0] + py * light[1] + ring_z * light[2]
    miss = px * px + py * py + ring_z * ring_z - along_light * along_light
    shade = 1 - smooth(1.02, 0.96, miss) * (along_light < 0)
    ring_light = ring_color * (density * (0.02 + 1.15 * shade))[..., None]

    behind = ring_z < pz
    image = image * (1 - np.clip(density, 0, 1))[..., None]
    image = image + ring_light * (behind | ~inside)[..., None]

    nx, ny, nz = px[inside], py[inside], pz[inside]
    latitude = nx * normal[0] + ny * normal[1] + nz * normal[2]
    wobble = fbm(noise, nx * 3, ny * 3 + 4, nz * 3, 5) * 0.02
    bands = fbm(noise, latitude * 7 + wobble * 6, wobble * 3, 0.3, 4)
    fine = fbm(noise, latitude * 34 + wobble * 22, 1.1, wobble * 8, 4)
    surface = mix(color(0.52, 0.37, 0.2), color(0.19, 0.115, 0.06), smooth(-0.4, 0.45, bands))
    surface = mix(surface, color(0.66, 0.54, 0.38), smooth(0.0, 0.5, fine) * 0.42)
    surface = mix(surface, color(0.24, 0.24, 0.27), smooth(0.72, 0.95, np.abs(latitude)) * 0.65)
    ndotl = nx * light[0] + ny * light[1] + nz * light[2]
    day = np.clip(ndotl, 0, 1) ** 0.8
    # sombra de los anillos sobre el planeta
    to_plane = -latitude / np.maximum(light[0] * normal[0] + light[1] * normal[1] + light[2] * normal[2], 1e-3)
    hit = np.stack((nx + light[0] * to_plane, ny + light[1] * to_plane, nz + light[2] * to_plane), axis=-1)
    shadow = ring_density(np.linalg.norm(hit, axis=-1), noise) * (to_plane > 0)
    lit = surface * (day * (1 - 0.72 * shadow) * 2.0)[..., None]
    image[inside] = lit + (np.exp(-nz * 6) * smooth(-0.1, 0.5, ndotl) * 0.12)[..., None] * color(1.0, 0.85, 0.6)

    front = (ring_z >= pz) & inside
    image[front] = image[front] * (1 - np.clip(density[front], 0, 1))[..., None] + ring_light[front]
    return image


def lunar_terrain(seed, size=2048):
    """Mapa de alturas de la Luna, con cráteres y sombras largas de un sol bajo que entra por la derecha."""
    rng = np.random.default_rng(seed)
    noise = Noise(seed)
    y, x = np.mgrid[0:size, 0:size].astype(np.float32) / size
    height = fbm(noise, x * 9, y * 9, 2.0, 7, 0.52) * 0.075 + ridged(noise, x * 3, y * 3, 5.0, 4) * 0.016
    yy, xx = np.mgrid[0:size, 0:size].astype(np.float32)
    for _ in range(420):
        radius = 7 * (70 / 7) ** (rng.random() ** 2.6)
        cx, cy = rng.random(2) * size
        reach = int(radius * 2.2)
        x0, x1 = max(int(cx) - reach, 0), min(int(cx) + reach, size)
        y0, y1 = max(int(cy) - reach, 0), min(int(cy) + reach, size)
        d = np.sqrt((xx[y0:y1, x0:x1] - cx) ** 2 + (yy[y0:y1, x0:x1] - cy) ** 2) / radius
        relief = -(1 - d * d) * (d < 1) + np.exp(-((d - 1) / 0.2) ** 2) * 0.5 * (d < 2)
        height[y0:y1, x0:x1] += relief * radius / size * 0.34
    slope_y, slope_x = np.gradient(height, 1 / size)
    sun = normalize(0.86, -0.2, 0.46)
    length = np.sqrt(slope_x ** 2 + slope_y ** 2 + 1)
    shade = np.clip((-slope_x * sun[0] - slope_y * sun[1] + sun[2]) / length, 0, 1)
    # recorre cada fila desde el sol: lo que queda por debajo de la línea de sombra no recibe luz
    drop = sun[2] / sun[0] / size
    line = height[:, -1].copy()
    lit = np.ones((size, size), dtype=np.float32)
    for column in range(size - 2, -1, -1):
        line = np.maximum(line - drop, height[:, column])
        lit[:, column] = 0.1 + 0.9 * smooth(0.002, 0.0, line - height[:, column])
    albedo = 0.36 + 0.13 * fbm(noise, x * 24, y * 24, 8.0, 5)
    return height, np.clip(shade * lit, 0, 1) * albedo


def scene_earthrise():
    x, y = grid()
    light = normalize(0.84, -0.14, 0.52)
    planet, cover = earth(x, y, 346, 88, 50, light, 11, spin=0.3, detail=6, sun=0.85)
    h, w = y.shape
    image = stars(41, 240) * (1 - cover[..., None]) + planet

    height, shaded = lunar_terrain(42)
    size = height.shape[0]
    horizon, focal, eye = int(196 * SCALE), 0.5 * w, 0.042
    depths = np.geomspace(0.01, 0.9, 1300, dtype=np.float32)
    origin_x, origin_y = 0.46, 0.04
    ground = height[int(origin_y * size), int(origin_x * size)]
    terrain = np.full((h, w), -1.0, dtype=np.float32)
    for column in range(w):
        offset = (column - w / 2) / focal
        mx = ((origin_x + offset * depths) % 1 * (size - 1)).astype(np.int32)
        my = ((origin_y + depths) % 1 * (size - 1)).astype(np.int32)
        rows = horizon + (ground + eye - height[my, mx]) / depths * focal * 0.7
        lowest = h
        for row, value in zip(rows, shaded[my, mx]):
            top = max(int(row), 0)
            if top < lowest:
                terrain[top:lowest, column] = value
                lowest = top
    covered = terrain >= 0
    painted = np.maximum(terrain, 0)[..., None] * color(1.0, 0.97, 0.93) * 0.8
    fade = smooth(0, 1, (np.arange(h, dtype=np.float32)[:, None] - horizon + 8 * SCALE) / (140 * SCALE))
    painted = painted * (1 - 0.72 * fade)[..., None]
    return image * (~covered)[..., None] + painted


def scene_nebula():
    x, y = grid()
    u, v = x / 175, y / 175
    a, b, c = Noise(51), Noise(52), Noise(53)
    warp_x, warp_y = fbm(b, u, v, 1, 4), fbm(c, u + 7, v, 2, 4)
    gas = fbm(a, u + warp_x * 1.1, v + warp_y * 1.1, 3, 6, 0.47)
    second = fbm(b, u * 1.3 - warp_y, v * 1.3 + warp_x, 5, 6, 0.47)
    third = fbm(c, u * 1.6 + warp_x, v * 1.6 - warp_y, 7, 6, 0.47)
    lanes = smooth(0.25, 0.8, ridged(a, u * 1.1 + warp_y * 0.8, v * 1.1 - warp_x * 0.8, 11, 5, 0.47))

    center = np.exp(-(((x - 305) / 190) ** 2 + ((y - 155) / 125) ** 2))
    field = 0.1 + center
    image = np.zeros(x.shape + (3,), dtype=np.float32) + color(0.0018, 0.0014, 0.007)
    image = image + (smooth(-0.45, 0.55, gas) ** 1.7 * field)[..., None] * color(0.075, 0.025, 0.42)
    image = image + (smooth(-0.3, 0.6, second) ** 2.2 * field)[..., None] * color(0.62, 0.05, 0.3)
    image = image + (smooth(-0.2, 0.6, third) ** 2.6 * field)[..., None] * color(0.03, 0.42, 0.5)
    glow = smooth(-0.15, 0.6, gas + second * 0.7) ** 2.4 * center ** 2
    image = image + glow[..., None] * color(1.0, 0.68, 0.46) * 1.15
    image = image * (1 - 0.62 * lanes * smooth(-0.2, 0.4, third + 0.1))[..., None]
    image = image + stars(54, 420)
    return bloom(image, 0.7, 3.5 * SCALE, 0.4)


def scene_galaxy():
    x, y = grid()
    cx, cy = 318, 150
    angle = np.radians(-24)
    dx, dy = x - cx, y - cy
    across = (dx * np.cos(angle) + dy * np.sin(angle)) / 178
    along = (-dx * np.sin(angle) + dy * np.cos(angle)) / (178 * 0.46)
    r = np.sqrt(across * across + along * along) + 1e-4
    phi = np.arctan2(along, across)
    a, b = Noise(61), Noise(62)
    flow = fbm(b, across * 3, along * 3, 1, 5)
    twist = np.log(r + 0.05) / np.tan(np.radians(19))
    arms = (0.5 + 0.5 * np.cos(2 * (phi - twist) + flow * 1.3)) ** 2.4
    clumps = smooth(-0.15, 0.4, fbm(a, across * 7, along * 7, 3, 6, 0.56))
    disk = np.exp(-r / 0.46) * smooth(1.3, 0.75, r)
    dust = smooth(0.25, 0.75, arms) * smooth(0.0, 0.5, fbm(b, across * 9, along * 9, 4, 5) + 0.15)

    blue = color(0.26, 0.42, 1.0)
    warm = color(1.0, 0.72, 0.42)
    tone = mix(warm, blue, smooth(0.08, 0.55, r))
    image = tone * ((arms * (0.35 + clumps) + 0.18) * disk * 3.4)[..., None]
    pink = (smooth(0.28, 0.5, fbm(a, across * 22, along * 22, 6, 4)) * arms * disk * 3.5)[..., None]
    image = image + pink * color(1.0, 0.2, 0.42)
    image = image * (1 - 0.5 * np.roll(dust, 2 * SCALE, axis=0))[..., None]
    core = np.exp(-(r / 0.1) ** 1.25)[..., None]
    image = image + core * color(1.0, 0.82, 0.58) * 5.0
    image = image + stars(63, 300)
    return bloom(image, 0.8, 4 * SCALE, 0.5)


def scene_aurora():
    x, y = grid()
    u, v = x / WIDTH, y / HEIGHT
    a, b = Noise(81), Noise(82)
    horizon = 0.73
    sky = mix(color(0.001, 0.003, 0.014), color(0.004, 0.022, 0.034), smooth(0.05, horizon, v))
    image = sky + stars(83, 300)

    def curtain(base, swing, height, strength, shift):
        line = base + swing * fbm(a, u * 2.1 + shift, 0.4, shift, 4)
        rays = 0.3 + 0.7 * smooth(-0.35, 0.5, fbm(b, u * 46 + fbm(a, u * 3, 0.9, shift, 3) * 10, shift, 0.2, 4))
        lift = line - v
        body = np.where(lift > 0, np.exp(-lift / height), np.exp(lift / 0.016))
        tint = mix(color(0.06, 0.95, 0.42), color(0.42, 0.1, 0.8), smooth(0.05, 0.32, lift))
        return tint * (body * rays * strength)[..., None]

    light = curtain(0.53, 0.26, 0.24, 0.5, 1.3) + curtain(0.42, 0.3, 0.17, 0.26, 4.1) + curtain(0.3, 0.26, 0.13, 0.14, 7.7)
    image = image + light + blur(light, 7 * SCALE) * 0.45

    for base, swing, tint, seed, width in (
        (0.6, 0.2, color(0.0052, 0.015, 0.023), 3.0, 2.2),
        (0.665, 0.14, color(0.0024, 0.0072, 0.012), 6.0, 3.4),
        (0.715, 0.07, color(0.0008, 0.0024, 0.0045), 9.0, 5.5),
    ):
        ridge = base - swing * (ridged(a, u * width, 0.2, seed, 6, 0.5) - 0.3)
        rock = (v >= ridge) & (v < horizon)
        snow = smooth(0.05, 0.0, v - ridge) * smooth(0.0, 0.3, fbm(b, u * 34, v * 34, seed, 4))
        image[rock] = (tint + snow[..., None] * tint * 2.4)[rock]

    water = v >= horizon
    ripple = fbm(a, u * 5, v * 70, 3, 3) * 0.05 * np.sqrt(np.maximum(v - horizon + 0.015, 0))
    row = np.clip(((2 * horizon - v) * y.shape[0]).astype(np.int32), 0, y.shape[0] - 1)
    column = np.clip(((u + ripple) * x.shape[1]).astype(np.int32), 0, x.shape[1] - 1)
    image[water] = image[row[water], column[water]] * 0.42
    return bloom(image, 0.85, 3 * SCALE, 0.25)


def scene_blackhole():
    """Rayos de luz curvados de verdad por la gravedad de un agujero negro de Schwarzschild, con radio 1."""
    scale = 2
    h, w = HEIGHT * scale, WIDTH * scale
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    field = 0.33
    center_x, center_y = 0.6 * w, 0.4 * h
    sx, sy = (x + 0.5 - center_x) / (h / 2) * field, -(y + 0.5 - center_y) / (h / 2) * field
    distance, pitch = 30.0, np.radians(7.5)
    eye = np.array([0, np.sin(pitch), -np.cos(pitch)], dtype=np.float32) * distance
    forward = -eye / distance
    right = np.array([1, 0, 0], dtype=np.float32)
    up = np.cross(forward, right)
    direction = forward[None, None, :] + sx[..., None] * right + sy[..., None] * up
    direction = (direction / np.linalg.norm(direction, axis=-1, keepdims=True)).reshape(-1, 3).astype(np.float32)
    position = np.tile(eye, (direction.shape[0], 1)).astype(np.float32)
    h2 = np.sum(np.cross(position, direction) ** 2, axis=-1)

    light = np.zeros((direction.shape[0], 3), dtype=np.float32)
    through = np.ones(direction.shape[0], dtype=np.float32)
    active = np.arange(direction.shape[0])
    streaks, inner, outer = Noise(91), 2.6, 12.5
    for _ in range(620):
        p, d = position[active], direction[active]
        r = np.linalg.norm(p, axis=-1)
        step = np.clip(0.11 * r, 0.03, 1.6)[..., None]
        accel = -1.5 * h2[active, None] * p / (r[..., None] ** 5)
        moved_d = d + accel * step
        moved_p = p + moved_d * step
        crossing = (p[:, 1] * moved_p[:, 1]) < 0
        if crossing.any():
            f = (p[crossing, 1] / (p[crossing, 1] - moved_p[crossing, 1]))[..., None]
            hit = p[crossing] + (moved_p[crossing] - p[crossing]) * f
            radius = np.linalg.norm(hit, axis=-1)
            theta = np.arctan2(hit[:, 2], hit[:, 0])
            wisps = fbm(streaks, np.cos(theta) * 2.6 + radius * 0.2, np.sin(theta) * 2.6 - radius * 0.2, radius * 1.5, 5)
            alpha = smooth(inner, inner + 0.6, radius) * smooth(outer, outer - 7.5, radius)
            alpha = np.clip(alpha * (0.72 + wisps * 1.3), 0, 1)
            speed = np.minimum(np.sqrt(0.5 / radius), 0.62)
            tangent = np.stack((-np.sin(theta), np.zeros_like(theta), np.cos(theta)), axis=-1)
            toward = np.sum(tangent * -moved_d[crossing] / np.linalg.norm(moved_d[crossing], axis=-1, keepdims=True), axis=-1)
            shift = np.sqrt(1 - speed * speed) / (1 - speed * toward)
            heat = (inner / radius) ** 1.7 * shift ** 3.2 * 1.9
            tint = mix(color(1.0, 0.24, 0.045), color(1.0, 0.74, 0.46), np.clip(heat / 2.6, 0, 1))
            index = active[crossing]
            light[index] += (through[index] * alpha * heat)[..., None] * tint
            through[index] *= 1 - alpha
        position[active], direction[active] = moved_p, moved_d
        r = np.linalg.norm(moved_p, axis=-1)
        keep = (r > 1.02) & ~((r > 34) & (np.sum(moved_p * moved_d, axis=-1) > 0))
        through[active[r <= 1.02]] = 0
        active = active[keep]
        if active.size == 0:
            break
    image = light.reshape(h, w, 3) + stars(92, 260, scale=scale) * through.reshape(h, w, 1)
    return bloom(image, 1.2, 5 * scale, 0.4), scale


def over(image, mask, tint):
    return image * (1 - mask[..., None]) + tint * mask[..., None]


def painted(draw):
    """Máscara dibujada con Pillow a la escala de trabajo. Se suaviza sola al reducir la imagen."""
    picture = Image.new("L", (WIDTH * SCALE, HEIGHT * SCALE), 0)
    draw(ImageDraw.Draw(picture), SCALE)
    return np.asarray(picture, dtype=np.float32) / 255


def from_srgb(picture):
    return np.power(np.asarray(picture, dtype=np.float32)[..., :3] / 255, 2.2)


def line_noise(noise, u, frequency, seed, octaves=5):
    return fbm(noise, u * frequency, np.full_like(u, 0.37), np.full_like(u, seed), octaves)


def scene_synthwave():
    x, y = grid()
    u, v = x / WIDTH, y / HEIGHT
    horizon, cx, cy, radius = 0.62, 312, 112, 84
    sky = mix(color(0.01, 0.003, 0.04), color(0.2, 0.012, 0.3), smooth(0.0, 0.42, v))
    sky = mix(sky, color(0.95, 0.13, 0.28), smooth(0.34, horizon, v) ** 1.6)
    image = sky + stars(101, 240) * smooth(0.5, 0.1, v)[..., None]

    d = np.sqrt((x - cx) ** 2 + (y - cy) ** 2) / radius
    image = image + (np.exp(-np.maximum(d - 1, 0) / 0.55) * 0.3)[..., None] * color(1.0, 0.16, 0.42)
    band = (y - cy) / radius
    gaps = ((band * 8.5) % 1 < smooth(-0.1, 1.0, band) * 0.66) & (band > -0.1)
    sun = mix(color(1.0, 0.82, 0.16), color(1.0, 0.07, 0.4), smooth(-0.85, 0.85, band)) * 1.9
    image = over(image, (d < 1) & ~gaps, sun)

    a = Noise(102)
    valley = smooth(50, 190, np.abs(x - cx))
    far = horizon - (line_noise(a, u, 4.0, 1.0, 5) + 0.42) * 0.24 * valley - 0.004
    near = horizon - (line_noise(a, u, 7.0, 5.0, 4) + 0.38) * 0.15 * smooth(90, 240, np.abs(x - cx))
    image = over(image, (v > far) & (v < horizon), color(0.045, 0.004, 0.1))
    image = over(image, (v > near) & (v < horizon), color(0.012, 0.0, 0.035))

    floor = v >= horizon
    depth = 1 / np.maximum(v - horizon, 1e-4)
    across = (x - cx) / WIDTH * depth * 2.4
    along = depth * 0.34
    near_x = np.abs((across + 0.5) % 1 - 0.5)
    near_z = np.abs((along + 0.5) % 1 - 0.5)
    step_x, step_z = 2.4 * depth / WIDTH, 0.34 * depth * depth / HEIGHT
    lines = np.maximum(smooth(1.3, 0.35, near_x / step_x), smooth(1.3, 0.35, near_z / step_z))
    fade = smooth(0.0, 0.09, v - horizon)
    ground = mix(color(0.28, 0.02, 0.2), color(0.008, 0.0, 0.03), smooth(0.0, 0.16, v - horizon))
    ground = ground + (lines * fade * 1.1)[..., None] * color(1.0, 0.08, 0.78)
    image[floor] = ground[floor]
    return bloom(image, 0.8, 3.5 * SCALE, 0.55)


def scene_cyberpunk():
    rng = np.random.default_rng(111)
    s = SCALE
    x, y = grid()
    u, v = x / WIDTH, y / HEIGHT
    street = 262
    image = mix(color(0.008, 0.004, 0.03), color(0.16, 0.022, 0.2), smooth(0.0, 0.8, v))
    haze = np.exp(-(((x - 345) / 150) ** 2 + ((y - 128) / 90) ** 2))
    image = image + haze[..., None] * color(0.32, 0.05, 0.3)
    neon = np.zeros_like(image)
    palette = [color(0.05, 0.85, 1.0), color(1.0, 0.08, 0.55), color(1.0, 0.62, 0.12), color(0.5, 0.2, 1.0)]
    layers = [
        (color(0.1, 0.03, 0.17), 24, 52, 58, 150, 0.05, 0.4),
        (color(0.035, 0.016, 0.085), 30, 64, 92, 196, 0.08, 0.75),
        (color(0.008, 0.006, 0.024), 40, 84, 128, 248, 0.09, 1.15),
    ]
    for wall, narrow, wide, low, high, chance, power in layers:
        left = -int(rng.integers(0, 20))
        while left < WIDTH:
            width = int(rng.integers(narrow, wide))
            top = street - int(rng.integers(low, high))
            image[top * s:street * s, max(left, 0) * s:(left + width) * s] = wall
            cell_w, cell_h = 4, 5
            for row in range(top + 4, street - 5, cell_h):
                for column in range(left + 3, left + width - 4, cell_w):
                    if column >= 0 and rng.random() < chance:
                        tint = palette[int(rng.integers(0, 3))] if rng.random() < 0.3 else color(1.0, 0.72, 0.36)
                        image[row * s:(row + 3) * s, column * s:(column + 2) * s] = tint * power * float(rng.uniform(0.35, 1.0))
            if rng.random() < 0.62 and width > 34:
                tint = palette[int(rng.integers(0, 4))]
                bar = left + int(rng.integers(5, width - 12))
                start = top + int(rng.integers(6, 30))
                for sign in range(int(rng.integers(3, 8))):
                    y0 = start + sign * 10
                    pattern = rng.random((3, 3)) < 0.55
                    for i in range(3):
                        for j in range(3):
                            if pattern[i, j] and y0 + 9 < street and bar >= 0:
                                neon[(y0 + i * 3) * s:(y0 + i * 3 + 2) * s, (bar + j * 3) * s:(bar + j * 3 + 2) * s] = tint * power * 2.6
            left += width + int(rng.integers(0, 5))
    image = image + neon + blur(neon, 5 * s) * 1.2
    water = v >= street / HEIGHT
    a = Noise(112)
    ripple = fbm(a, u * 9, v * 90, 3, 3) * 0.012
    row = np.clip(((2 * street / HEIGHT - v) * y.shape[0]).astype(np.int32), 0, y.shape[0] - 1)
    column = np.clip(((u + ripple) * x.shape[1]).astype(np.int32), 0, x.shape[1] - 1)
    smeared = blur(image, 3.2 * s)
    image[water] = smeared[row[water], column[water]] * 0.42 + color(0.004, 0.003, 0.012)

    def rain(draw, scale):
        for _ in range(420):
            px, py = rng.uniform(-20, WIDTH), rng.uniform(-20, HEIGHT)
            size = rng.uniform(5, 15)
            draw.line((px * scale, py * scale, (px + size * 0.22) * scale, (py + size) * scale), fill=int(rng.uniform(50, 150)), width=1)

    image = image + painted(rain)[..., None] * color(0.42, 0.5, 0.75) * 0.3
    return bloom(image, 0.9, 2.5 * s, 0.4)


def scene_retro():
    """Pixel art de verdad: se pinta en 160×107 con pocos colores y se amplía sin suavizar."""
    w, h, zoom = 160, 107, 3
    rng = np.random.default_rng(121)
    bayer = np.array([[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]) / 16
    bands = np.array([[42, 60, 136], [58, 92, 178], [84, 134, 214], [126, 178, 232], [186, 216, 240]], dtype=np.uint8)
    picture = np.zeros((h, w, 3), dtype=np.uint8)
    for row in range(h):
        for column in range(w):
            level = row / 84 * (len(bands) - 1)
            base = min(int(level + bayer[row % 4, column % 4]), len(bands) - 1)
            picture[row, column] = bands[base]

    def disk(cx, cy, r, tint, squash=1.0):
        rows, columns = np.mgrid[0:h, 0:w]
        picture[((columns - cx) ** 2 + ((rows - cy) / squash) ** 2) <= r * r] = tint

    disk(128, 22, 11, (255, 226, 130))
    disk(128, 22, 8, (255, 244, 190))
    for cx, cy, size in ((82, 14, 1.0), (24, 40, 0.8), (108, 28, 0.6)):
        for dx, dy, r in ((0, 0, 7), (-8, 2, 5), (8, 2, 5), (14, 3, 3), (-13, 3, 3)):
            disk(cx + dx * size, cy + dy * size, r * size + 1, (150, 190, 228))
        for dx, dy, r in ((0, -1, 7), (-8, 1, 5), (8, 1, 5), (14, 2, 3), (-13, 2, 3)):
            disk(cx + dx * size, cy + dy * size, r * size, (250, 250, 255))

    def ridge(top, swing, frequency, phase, tint, edge):
        for column in range(w):
            height = top + swing * (np.sin(column * frequency + phase) * 0.6 + np.sin(column * frequency * 2.7 + phase * 2) * 0.4)
            start = int(height // 2 * 2)
            picture[start:, column] = tint
            picture[start:start + 2, column] = edge

    ridge(58, 11, 0.075, 0.4, (86, 92, 168), (124, 130, 200))
    ridge(70, 7, 0.11, 2.1, (60, 124, 116), (84, 156, 128))
    ridge(80, 4, 0.16, 4.0, (54, 150, 84), (104, 200, 96))

    # castillo sobre la colina
    stone, shade, dark = (150, 150, 168), (108, 108, 132), (52, 48, 76)
    picture[44:76, 104:136] = stone
    picture[44:76, 128:136] = shade
    picture[34:76, 100:112] = stone
    picture[34:76, 128:140] = shade
    for column in list(range(100, 112, 4)) + list(range(128, 140, 4)):
        picture[31:34, column:column + 2] = stone
    for column in range(104, 136, 4):
        picture[41:44, column:column + 2] = stone
    picture[62:76, 114:124] = dark
    picture[44:48, 104:108] = dark
    picture[44:48, 118:121] = dark
    picture[24:31, 105] = (70, 56, 60)
    picture[24:28, 106:112] = (226, 72, 72)

    ground = 88
    for row in range(ground, h):
        for column in range(w):
            light = ((column // 8) + (row - ground) // 8) % 2 == 0
            picture[row, column] = (148, 92, 56) if light else (126, 76, 46)
    for column in range(0, w, 8):
        picture[ground + 6:h, column] = (104, 62, 40)
    for row in range(ground + 6, h, 8):
        picture[row, :] = (104, 62, 40)
    picture[ground:ground + 3] = (116, 208, 84)
    for column in range(w):
        drip = 3 + ((column * 7) % 5 == 0) + ((column * 3) % 7 == 0) * 2
        picture[ground + 3:ground + 2 + drip, column] = (72, 164, 72)
    picture[ground - 1, ::3] = (160, 232, 112)

    for bx, by, count in ((24, 66, 4), (60, 56, 3)):
        for block in range(count):
            left = bx + block * 8
            picture[by:by + 8, left:left + 8] = (196, 104, 60)
            picture[by, left:left + 8] = (236, 156, 104)
            picture[by:by + 8, left + 7] = (116, 60, 40)
            picture[by + 3, left:left + 8] = (116, 60, 40)
            picture[by + 7, left:left + 8] = (116, 60, 40)
            picture[by:by + 3, left + 3] = (116, 60, 40)
            picture[by + 3:by + 7, left + 6] = (116, 60, 40)
    for cx, cy in ((30, 58), (40, 58), (50, 58), (66, 48), (76, 48)):
        picture[cy - 3:cy + 3, cx - 1:cx + 1] = (255, 210, 72)
        picture[cy - 2:cy + 2, cx - 2:cx + 2] = (255, 210, 72)
        picture[cy - 2:cy + 1, cx - 1] = (255, 246, 180)
    for cx in (10, 92, 146):
        picture[ground - 8:ground, cx - 1:cx + 2] = (112, 72, 48)
        disk(cx, ground - 13, 7, (44, 124, 76))
        disk(cx - 1, ground - 15, 5, (72, 168, 88))
        disk(cx - 2, ground - 16, 2, (120, 208, 112))
    big = np.repeat(np.repeat(picture, zoom, axis=0), zoom, axis=1)[:HEIGHT, :WIDTH]
    return Image.fromarray(big)


def scene_blocks():
    """Una isla de cubos flotando al atardecer, dibujada de atrás hacia delante."""
    s = SCALE
    x, y = grid()
    v = y / HEIGHT
    sky = mix(color(0.02, 0.05, 0.16), color(0.2, 0.13, 0.32), smooth(0.0, 0.55, v))
    sky = mix(sky, color(0.95, 0.34, 0.16), smooth(0.5, 1.0, v) ** 1.4)
    a = Noise(131)
    clouds = smooth(0.05, 0.4, fbm(a, x / 95, y / 30, 2, 5))
    sky = sky + (clouds * 0.3)[..., None] * color(0.95, 0.5, 0.42)
    canvas = Image.new("RGB", (WIDTH * s, HEIGHT * s))
    canvas.paste(Image.fromarray((np.power(np.clip(sky, 0, 1), 1 / 2.2) * 255).astype(np.uint8)))
    draw = ImageDraw.Draw(canvas)

    size, tw, th, tall, cx, cy = 15, 11.0, 5.5, 10.0, 322, 66
    rng = np.random.default_rng(132)
    n = Noise(133)
    tops = {}
    for i in range(size):
        for j in range(size):
            di, dj = (i - size / 2 + 0.5) / (size / 2), (j - size / 2 + 0.5) / (size / 2)
            edge = 1 - np.sqrt(di * di + dj * dj)
            if edge < 0.02 + 0.1 * float(fbm(n, i * 0.3, j * 0.3, 1.5, 3)):
                continue
            hill = float(fbm(n, i * 0.16, j * 0.16, 0.5, 4))
            tops[(i, j)] = (int(np.clip(edge * 4.2 + hill * 6.5 + 0.7, 0, 7)), max(int(edge * 8.5 + 1 + rng.integers(0, 3)), 1))

    def cube(i, j, k, top, left, right, scale=1.0, lift=0.0):
        px = cx + (i - j) * tw
        py = cy + (i + j) * th - k * tall - lift
        w, h, z = tw * scale, th * scale, tall * scale
        draw.polygon([(a0 * s, b0 * s) for a0, b0 in ((px, py), (px + w, py + h), (px, py + 2 * h), (px - w, py + h))], fill=top)
        draw.polygon([(a0 * s, b0 * s) for a0, b0 in ((px - w, py + h), (px, py + 2 * h), (px, py + 2 * h + z), (px - w, py + h + z))], fill=left)
        draw.polygon([(a0 * s, b0 * s) for a0, b0 in ((px + w, py + h), (px, py + 2 * h), (px, py + 2 * h + z), (px + w, py + h + z))], fill=right)

    def tint(base, spread=8):
        shift = int(rng.integers(-spread, spread))
        return tuple(int(np.clip(c + shift, 0, 255)) for c in base)

    trees = [(5, 5), (7, 4), (9, 7), (6, 9), (10, 5), (8, 10), (4, 8)]
    for total in range(size * 2):
        for i in range(size):
            j = total - i
            if (i, j) not in tops:
                continue
            height, under = tops[(i, j)]
            for k in range(-under, height):
                cube(i, j, k, tint((104, 104, 112)), (70, 66, 78), (88, 84, 96))
            if height <= 0:
                cube(i, j, 0, tint((58, 130, 198), 6), (36, 92, 150), (46, 108, 170))
                continue
            sand = height == 1
            top = tint((214, 188, 124) if sand else (112, 178, 76))
            cube(i, j, height, top, (176, 150, 96) if sand else (110, 78, 54), (196, 170, 112) if sand else (134, 96, 66))
            if (i, j) in trees and not sand:
                for k in range(height + 1, height + 4):
                    cube(i, j, k, (112, 78, 52), (84, 58, 40), (100, 70, 48), 0.42, -6)
                for di, dj, dk in ((0, 0, 4), (-1, 0, 4), (0, -1, 4), (1, 0, 4), (0, 1, 4), (0, 0, 5), (-1, 0, 5), (0, -1, 5), (0, 0, 6)):
                    cube(i + di, j + dj, height + dk, tint((78, 152, 72), 12), (46, 104, 56), (60, 124, 64))
    return bloom(from_srgb(canvas), 1.5, 3 * s, 0.1)


def scene_dungeon():
    x, y = grid()
    a, b = Noise(141), Noise(142)
    rng = np.random.default_rng(143)
    floor_y, door_x, door_w, door_top = 266, 346, 56, 104
    row = np.floor(y / 26)
    shift = (row % 2) * 24
    column = np.floor((x + shift) / 48)
    fx, fy = (x + shift) / 48 - column, y / 26 - row
    mortar = 1 - smooth(0.0, 0.05, fx) * smooth(1.0, 0.95, fx) * smooth(0.0, 0.09, fy) * smooth(1.0, 0.91, fy)
    table = rng.random((64, 64))
    tone = table[(row.astype(np.int32)) % 64, (column.astype(np.int32)) % 64]
    grain = fbm(a, x / 9, y / 9, 1, 5)
    height = (1 - mortar) * (0.7 + 0.3 * tone) + grain * 0.3
    dy, dx = np.gradient(blur(height, 1.0 * SCALE))
    bump = 18 * SCALE
    wall = (0.2 + 0.22 * tone + 0.12 * grain) * (1 - 0.72 * mortar)
    albedo = mix(color(0.42, 0.37, 0.34), color(0.22, 0.25, 0.3), tone) * wall[..., None] * 2.2

    torches = ((256, 118), (436, 118))
    image = np.zeros(x.shape + (3,), dtype=np.float32)
    for tx, ty in torches:
        lx, ly, lz = tx - x, ty - y, 46.0
        length = np.sqrt(lx * lx + ly * ly + lz * lz)
        nx, ny, nz = -dx * bump, -dy * bump, 1.0
        norm = np.sqrt(nx * nx + ny * ny + nz * nz)
        shade = np.clip((nx * lx + ny * ly + nz * lz) / (norm * length), 0, 1)
        falloff = 1 / (1 + (length / 62) ** 2.2)
        image = image + albedo * (shade * falloff * 4.2)[..., None] * color(1.0, 0.52, 0.22)
    image = image + albedo * color(0.02, 0.032, 0.05)

    floor = y >= floor_y
    depth = 90 / np.maximum(y - floor_y + 12, 1)
    px, pz = (x - door_x) * depth / 50, depth * 2.4
    tile = smooth(0.0, 0.06, np.abs((px % 1) - 0.5) - 0.0) * 0 + 1
    grout = np.maximum(smooth(0.05, 0.0, np.abs((px + 0.5) % 1 - 0.5)), smooth(0.08, 0.0, np.abs((pz + 0.5) % 1 - 0.5)))
    stone = (0.11 + 0.08 * fbm(b, px * 3, pz * 3, 1, 4)) * (1 - 0.7 * grout) * tile
    light = sum(1 / (1 + ((x - tx) ** 2 + ((y - floor_y) * 2.6) ** 2) / 12000) for tx, _ in torches)
    ground = (stone * light * 1.15)[..., None] * color(1.0, 0.54, 0.24) + color(0.002, 0.003, 0.005)
    image[floor] = ground[floor]

    arch = (np.abs(x - door_x) < door_w) & (y > door_top + door_w) & (y < floor_y)
    round_top = ((x - door_x) ** 2 + (y - door_top - door_w) ** 2) < door_w ** 2
    door = arch | round_top
    frame = (np.abs(x - door_x) < door_w + 7) & (y > door_top + door_w) & (y < floor_y)
    frame = frame | (((x - door_x) ** 2 + (y - door_top - door_w) ** 2) < (door_w + 7) ** 2)
    image = over(image, frame, color(0.05, 0.04, 0.04))
    behind = np.exp(-((x - door_x) ** 2 + (y - 210) ** 2) / 2600)[..., None] * color(0.02, 0.09, 0.14)
    bars = (np.abs(((x - door_x) % 17) - 8.5) < 1.3) | (np.abs(((y - door_top) % 40) - 20) < 1.4)
    interior = behind + color(0.002, 0.003, 0.006)
    image[door] = interior[door]
    image = over(image, door & bars, color(0.012, 0.011, 0.012))

    flames = np.zeros_like(image)
    for tx, ty in torches:
        image = over(image, (np.abs(x - tx) < 3.4) & (y > ty + 12) & (y < ty + 46), color(0.012, 0.008, 0.006))
        image = over(image, (np.abs(x - tx) < 7) & (np.abs(y - ty - 12) < 3.4), color(0.03, 0.02, 0.012))
        warp = fbm(a, x / 7, y / 11 - ty, tx, 4) * 5
        fy2 = (ty + 10 - y) / 34
        width = 8.5 * np.clip(fy2 * 3.2, 0, 1) ** 0.5 * np.clip(1 - fy2, 0, 1) ** 0.9
        body = smooth(1.0, 0.3, np.abs(x - tx + warp * fy2) / np.maximum(width, 0.01)) * (fy2 > 0) * (fy2 < 1)
        fire = mix(color(1.0, 0.2, 0.025), color(1.0, 0.8, 0.34), smooth(0.2, 0.85, body * (1 - fy2 * 0.65)))
        flames = flames + fire * (body * 3.4)[..., None]
    for _ in range(46):
        tx, ty = torches[int(rng.integers(0, 2))]
        ex, ey = tx + rng.normal(0, 15), ty - rng.uniform(16, 96)
        spark = np.exp(-((x - ex) ** 2 + (y - ey) ** 2) / 0.8)
        flames = flames + spark[..., None] * color(1.0, 0.42, 0.1) * float(rng.uniform(0.6, 2.4))
    image = image + flames
    edge = np.exp(-(((x - 340) / 280) ** 2 + ((y - 165) / 215) ** 2))
    return bloom(image * edge[..., None], 0.9, 4 * SCALE, 0.65)


def scene_cavern():
    x, y = grid()
    u, v = x / WIDTH, y / HEIGHT
    a, b = Noise(151), Noise(152)
    rng = np.random.default_rng(153)
    glow = np.exp(-(((x - 322) / 175) ** 2 + ((y - 150) / 105) ** 2))
    image = mix(color(0.003, 0.012, 0.024), color(0.09, 0.3, 0.42), glow)
    image = image + (smooth(-0.1, 0.5, fbm(a, x / 70, y / 45, 1, 5)) * glow * 0.2)[..., None] * color(0.4, 0.75, 0.85)
    fog = color(0.05, 0.17, 0.25)
    shadow = color(0.0015, 0.005, 0.009)
    for layer in range(5):
        depth = layer / 4
        tint = mix(fog, shadow, depth ** 0.7)
        drip = np.abs(line_noise(a, u, 11 + layer * 5, layer + 1.0, 4))
        ceiling = 0.05 + 0.1 * depth + (0.2 + depth * 0.14) * smooth(0.28, 0.0, drip) * (line_noise(b, u, 5, layer + 2.0, 2) + 0.65)
        ceiling = ceiling + 0.05 * (line_noise(b, u, 2.3, layer + 3.0, 3) + 0.4)
        mounds = 0.92 - 0.09 * depth - (0.11 + depth * 0.07) * (line_noise(a, u, 4 + layer * 2, layer + 7.0, 5) + 0.45)
        image = over(image, (v < ceiling) | (v > mounds), tint)

    glowing = np.zeros_like(image)

    def stems(draw, scale):
        for lx, ly, _ in LANTERNS:
            draw.line((lx * scale, 0, lx * scale, (ly - 5) * scale), fill=255, width=max(scale // 2, 1))
        for mx, my, size in MUSHROOMS:
            draw.line((mx * scale, my * scale, mx * scale, (my - size) * scale), fill=255, width=max(scale, 1))

    LANTERNS = ((212, 92, 4.6), (262, 58, 3.4), (374, 106, 5.2), (428, 66, 3.8), (328, 42, 3.0))
    MUSHROOMS = ((204, 288, 13), (216, 292, 9), (392, 286, 15), (406, 292, 10), (444, 280, 12), (300, 298, 8))
    image = over(image, painted(stems), shadow)
    for lx, ly, size in LANTERNS:
        d = (x - lx) ** 2 + (y - ly) ** 2
        glowing = glowing + np.exp(-d / (size * size * 1.2))[..., None] * color(1.0, 0.86, 0.56) * 3.2
        glowing = glowing + np.exp(-d / (size * size * 26))[..., None] * color(1.0, 0.62, 0.3) * 0.34
    for mx, my, size in MUSHROOMS:
        d = (x - mx) ** 2 + ((y - my + size) * 1.9) ** 2
        cap = (d < size * size * 0.42) & (y < my - size + 1)
        glowing = glowing + cap[..., None] * color(0.14, 0.95, 0.9) * 1.8
        glowing = glowing + np.exp(-d / (size * size * 5))[..., None] * color(0.1, 0.8, 0.8) * 0.32
    for _ in range(120):
        sx, sy = rng.uniform(150, WIDTH), rng.uniform(30, 300)
        glowing = glowing + np.exp(-((x - sx) ** 2 + (y - sy) ** 2) / 0.75)[..., None] * color(0.5, 1.0, 0.95) * float(rng.uniform(0.2, 1.6))
    return bloom(image + glowing, 0.8, 3 * SCALE, 0.5)


def scene_sakura():
    s = SCALE
    x, y = grid()
    u, v = x / WIDTH, y / HEIGHT
    a = Noise(161)
    rng = np.random.default_rng(162)
    image = mix(color(0.03, 0.03, 0.14), color(0.5, 0.11, 0.26), smooth(0.0, 0.5, v))
    image = mix(image, color(1.0, 0.42, 0.2), smooth(0.42, 0.78, v) ** 1.3)
    cx, cy, radius = 330, 128, 74
    d = np.sqrt((x - cx) ** 2 + (y - cy) ** 2) / radius
    image = image + np.exp(-np.maximum(d - 1, 0) / 0.8)[..., None] * color(1.0, 0.4, 0.2) * 0.2
    image = over(image, d < 1, mix(color(1.0, 0.86, 0.6), color(1.0, 0.42, 0.26), smooth(-0.9, 0.9, (y - cy) / radius)) * 1.15)

    peak = 96 + np.abs(x - 300) * 0.62 + 5 * fbm(a, x / 24, 1.0, 2.0, 4) * smooth(0, 100, np.abs(x - 300))
    mountain = y > peak
    snow = y < 150 + 16 * fbm(a, x / 12, 3.0, 2.0, 3)
    tone = mix(color(0.2, 0.09, 0.28), color(0.5, 0.2, 0.3), smooth(100, 240, y))
    image = over(image, mountain, tone)
    image = over(image, mountain & snow, color(0.92, 0.72, 0.74))
    mist = smooth(175, 245, y)[..., None] * color(0.95, 0.42, 0.32)
    image = image + mist * 0.4

    for base, swing, tint, seed, frequency in (
        (0.76, 0.1, color(0.2, 0.07, 0.2), 1.0, 3.4),
        (0.83, 0.09, color(0.075, 0.03, 0.1), 2.0, 5.0),
        (0.9, 0.06, color(0.016, 0.008, 0.028), 3.0, 3.2),
    ):
        ridge = base - swing * (line_noise(a, u, frequency, seed, 5) + 0.4)
        image = over(image, v > ridge, tint)
        image = image + (smooth(0.09, 0.0, v - ridge) * (v > ridge) * 0.05)[..., None] * color(1.0, 0.45, 0.4)

    def gate(draw, scale):
        left, right, top, foot = 212, 300, 158, 270
        pillar = 6
        for px, lean in ((left, 3), (right, -3)):
            draw.polygon([((px - pillar + lean) * scale, top * scale), ((px + pillar + lean) * scale, top * scale), ((px + pillar + 1) * scale, foot * scale), ((px - pillar - 1) * scale, foot * scale)], fill=255)
        draw.polygon([((left - 26) * scale, (top - 22) * scale), ((left + 8) * scale, (top - 14) * scale), ((right - 8) * scale, (top - 14) * scale), ((right + 26) * scale, (top - 22) * scale), ((right + 22) * scale, (top - 11) * scale), ((left - 22) * scale, (top - 11) * scale)], fill=255)
        draw.rectangle(((left - 15) * scale, (top - 8) * scale, (right + 15) * scale, (top - 2) * scale), fill=255)
        draw.rectangle(((left - 10) * scale, (top + 16) * scale, (right + 10) * scale, (top + 22) * scale), fill=255)
        draw.rectangle((((left + right) / 2 - 4) * scale, (top - 8) * scale, ((left + right) / 2 + 4) * scale, (top + 16) * scale), fill=255)

    image = over(image, painted(gate), color(0.11, 0.008, 0.012))

    twigs, blossoms = [], []

    def grow(px, py, angle, length, width):
        ex, ey = px + np.cos(angle) * length, py + np.sin(angle) * length
        twigs.append((px, py, ex, ey, width))
        if width < 1.2:
            blossoms.append((ex, ey))
            return
        for _ in range(int(rng.integers(2, 4))):
            grow(ex, ey, angle + float(rng.normal(0, 0.55)), length * float(rng.uniform(0.62, 0.84)), width * 0.68)

    grow(490, 18, np.radians(172), 58, 7.5)
    grow(490, 74, np.radians(196), 46, 5.5)

    def branches(draw, scale):
        for x0, y0, x1, y1, width in twigs:
            draw.line((x0 * scale, y0 * scale, x1 * scale, y1 * scale), fill=255, width=max(int(width * scale), 1))

    image = over(image, painted(branches), color(0.016, 0.006, 0.012))

    def flowers(draw, scale):
        for bx, by in blossoms:
            for _ in range(int(rng.integers(5, 10))):
                px, py = bx + rng.normal(0, 8), by + rng.normal(0, 8)
                r = rng.uniform(1.4, 3.1)
                draw.ellipse(((px - r) * scale, (py - r) * scale, (px + r) * scale, (py + r) * scale), fill=int(rng.uniform(120, 255)))
        for _ in range(70):
            px, py = rng.uniform(150, WIDTH), rng.uniform(40, 305)
            r = rng.uniform(0.9, 2.0)
            draw.ellipse(((px - r * 1.3) * scale, (py - r) * scale, (px + r * 1.3) * scale, (py + r) * scale), fill=int(rng.uniform(90, 230)))

    image = over(image, painted(flowers), color(1.0, 0.56, 0.68))
    return bloom(image, 1.1, 2 * s, 0.25)


def scene_reef():
    x, y = grid()
    u, v = x / WIDTH, y / HEIGHT
    a, b = Noise(171), Noise(172)
    rng = np.random.default_rng(173)
    water = mix(color(0.012, 0.32, 0.42), color(0.002, 0.03, 0.09), smooth(0.0, 0.85, v) ** 0.8)
    slant = u + (v - 0.1) * 0.22
    rays = smooth(0.0, 0.5, fbm(a, slant * 15, 0.4, 0.5, 4)) * smooth(0.95, 0.05, v)
    image = water + (rays * 0.32)[..., None] * color(0.3, 0.9, 0.85)
    ripple = smooth(0.1, 0.5, ridged(b, x / 34, y / 12, 1, 4)) * smooth(0.16, 0.0, v)
    image = image + (ripple * 0.5)[..., None] * color(0.5, 1.0, 0.95)

    def fan(draw, scale, px, py, angle, length, width, spread, depth):
        ex, ey = px + np.cos(angle) * length, py + np.sin(angle) * length
        draw.line((px * scale, py * scale, ex * scale, ey * scale), fill=255, width=max(int(width * scale), 1))
        if depth > 0:
            for _ in range(int(rng.integers(2, 4))):
                fan(draw, scale, ex, ey, angle + float(rng.uniform(-spread, spread)), length * float(rng.uniform(0.66, 0.86)), width * 0.74, spread, depth - 1)

    far_ground = 0.86 - 0.05 * (line_noise(a, u, 3.2, 2.0, 4) + 0.4)
    image = over(image, v > far_ground, color(0.006, 0.05, 0.085))
    corals = ((240, 312, 15, color(0.9, 0.22, 0.32)), (300, 318, 19, color(1.0, 0.45, 0.14)), (368, 314, 17, color(0.62, 0.2, 0.75)), (434, 318, 22, color(0.95, 0.2, 0.42)), (186, 320, 12, color(0.9, 0.55, 0.16)))
    for px, py, length, tint in corals:
        mask = painted(lambda draw, scale: fan(draw, scale, px, py, -np.pi / 2, length, 5.5, 0.6, 5))
        image = over(image, mask, tint * 0.62)

    def weed(draw, scale):
        for start in (170, 212, 268, 340, 402, 462):
            for blade in range(5):
                bx = start + blade * 5 + rng.uniform(-3, 3)
                reach, phase = rng.uniform(46, 100), rng.uniform(0, 6)
                points = [((bx + np.sin(t * 5 + phase) * t * 11) * scale, (324 - t * reach) * scale) for t in np.linspace(0, 1, 16)]
                draw.line(points, fill=255, width=max(int(2 * scale), 1))

    image = over(image, painted(weed), color(0.012, 0.22, 0.13))
    near_ground = 0.95 - 0.05 * (line_noise(b, u, 5, 4.0, 4) + 0.4)
    image = over(image, v > near_ground, color(0.003, 0.018, 0.03))

    def fish(draw, scale):
        for _ in range(26):
            px, py = rng.normal(350, 62), rng.normal(120, 34)
            r = rng.uniform(4.0, 7.0)
            shape = [(px - r, py), (px - r * 0.1, py - r * 0.5), (px + r, py), (px + r * 1.6, py - r * 0.5), (px + r * 1.5, py), (px + r * 1.6, py + r * 0.5), (px + r, py), (px - r * 0.1, py + r * 0.5)]
            draw.polygon([(p * scale, q * scale) for p, q in shape], fill=int(rng.uniform(150, 255)))

    image = over(image, painted(fish), color(1.0, 0.72, 0.2))

    def bubbles(draw, scale):
        for _ in range(36):
            px, py, r = rng.uniform(170, WIDTH), rng.uniform(30, 290), rng.uniform(0.9, 2.7)
            draw.ellipse(((px - r) * scale, (py - r) * scale, (px + r) * scale, (py + r) * scale), outline=int(rng.uniform(90, 200)), width=max(scale // 2, 1))

    image = image + painted(bubbles)[..., None] * color(0.6, 0.95, 1.0) * 0.5
    return bloom(image, 0.9, 3 * SCALE, 0.3)


def scene_forest():
    x, y = grid()
    u, v = x / WIDTH, y / HEIGHT
    rng = np.random.default_rng(181)
    a = Noise(182)
    sun_x, sun_y = 372, 40
    distance = np.sqrt((x - sun_x) ** 2 + (y - sun_y) ** 2)
    fog = mix(color(0.75, 0.62, 0.36), color(0.05, 0.14, 0.15), smooth(0, 300, distance))
    image = fog.copy()
    angle = np.arctan2(y - sun_y, x - sun_x)
    rays = smooth(0.0, 0.5, fbm(a, angle * 7, 0.6, 0.5, 4)) * smooth(330, 20, distance)
    image = image + (rays * 0.36)[..., None] * color(1.0, 0.82, 0.5)

    def pines(draw, scale, ground, low, high, spacing):
        px = -20.0
        while px < WIDTH + 20:
            tall = rng.uniform(low, high)
            half = tall * rng.uniform(0.15, 0.2)
            levels = int(tall / 7) + 3
            left, right = [], []
            for level in range(levels + 1):
                t = level / levels
                top = ground - tall + t * tall * 0.94
                reach = half * t ** 0.85 * (rng.uniform(0.82, 1.1) if level % 2 else rng.uniform(0.5, 0.72)) + rng.uniform(-0.6, 0.6)
                left.append(((px - reach) * scale, top * scale))
                right.append(((px + reach) * scale, top * scale))
            draw.polygon(left + right[::-1], fill=255)
            draw.rectangle(((px - 1.6) * scale, (ground - tall * 0.08) * scale, (px + 1.6) * scale, 330 * scale), fill=255)
            px += rng.uniform(0.55, 1.4) * spacing

    rows = ((190, 40, 74, 10), (214, 62, 108, 14), (240, 86, 142, 20), (268, 112, 176, 30), (298, 150, 226, 46))
    for index, (ground, low, high, spacing) in enumerate(rows):
        depth = index / (len(rows) - 1)
        mask = painted(lambda draw, scale: pines(draw, scale, ground, low, high, spacing))
        tint = mix(fog, color(0.0045, 0.014, 0.012), 0.42 + 0.58 * depth ** 0.8)
        image = over(image, mask, tint)
        image = over(image, smooth(ground, ground + 12, y), tint)
        image = image + (smooth(ground - 70, ground, y) * (1 - depth) * 0.1)[..., None] * fog
    return image


def scene_rain():
    """Lluvia de símbolos inventados: mapas de bits al azar, no un alfabeto."""
    s = SCALE
    rng = np.random.default_rng(191)
    h, w = HEIGHT * s, WIDTH * s
    glyphs = rng.random((96, 7, 5)) < 0.52
    glyphs[:, :, 2] |= rng.random((96, 7)) < 0.4
    image = np.zeros((h, w, 3), dtype=np.float32)
    for pitch_x, pitch_y, dot, strength, chance in ((9, 11, 1, 0.1, 0.6), (15, 19, 2, 0.8, 0.42)):
        for column in range(0, WIDTH, pitch_x):
            if rng.random() > chance:
                continue
            head = int(rng.integers(2, HEIGHT // pitch_y + 8))
            length = int(rng.integers(7, 24))
            for row in range(max(head - length, 0), min(head, HEIGHT // pitch_y + 1)):
                fade = 1 - (head - 1 - row) / length
                tint = color(0.72, 1.0, 0.82) * 1.8 if row == head - 1 else color(0.02, 0.85, 0.28) * fade ** 1.7
                pattern = glyphs[int(rng.integers(0, 96))]
                for i in range(7):
                    for j in range(5):
                        if pattern[i, j]:
                            top, left = (row * pitch_y + i * dot) * s, (column + j * dot) * s
                            image[top:top + dot * s - 1, left:left + dot * s - 1] = tint * strength
    return bloom(image, 0.35, 2.2 * s, 0.7)


SCENES = {
    "orbit": scene_orbit,
    "mars": scene_mars,
    "saturn": scene_saturn,
    "earthrise": scene_earthrise,
    "nebula": scene_nebula,
    "galaxy": scene_galaxy,
    "aurora": scene_aurora,
    "blackhole": scene_blackhole,
    "synthwave": scene_synthwave,
    "cyberpunk": scene_cyberpunk,
    "retro": scene_retro,
    "blocks": scene_blocks,
    "dungeon": scene_dungeon,
    "cavern": scene_cavern,
    "sakura": scene_sakura,
    "reef": scene_reef,
    "forest": scene_forest,
    "rain": scene_rain,
}


def main():
    parser = argparse.ArgumentParser(description="Genera los fondos de los temas incluidos")
    parser.add_argument("scenes", nargs="*", choices=[[], *SCENES], default=[], help="escenas a generar; sin nada, todas")
    names = parser.parse_args().scenes or list(SCENES)
    for name in names:
        result = SCENES[name]()
        if isinstance(result, Image.Image):
            OUT.mkdir(parents=True, exist_ok=True)
            result.save(OUT / f"{name}.png", optimize=True)
            print(name, (OUT / f"{name}.png").stat().st_size // 1024, "KB")
            continue
        image, scale = result if isinstance(result, tuple) else (result, SCALE)
        finish(name, image, scale)


if __name__ == "__main__":
    main()
