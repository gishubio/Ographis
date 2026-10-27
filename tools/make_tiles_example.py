"""Writes examples/tiles: a small 3D Tiles tileset of three copies of examples/house/house.glb
on the made-up terrain, as Batched 3D Models with a name and a year for each house.
The heights are above the ellipsoid: the ground (RH 2000) plus 32 m, which the example model
takes away again with Move the heights by."""
import json, math, os, struct, subprocess

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, 'examples', 'tiles')
GEOID = 32.0
A, F = 6378137.0, 1 / 298.257223563
HOUSES = [  # name, built, SWEREF 99 TM east and north, ground in RH 2000, turned degrees
    ('Ekbacken', 1932, 418232.0, 6472462.0, 86.8, 25.0),
    ('Lindgården', 1958, 418250.0, 6472445.0, 85.5, 10.0),
    ('Björkhagen', 1974, 418268.0, 6472428.0, 84.9, -5.0),
]


def lonlat(east, north):
    out = subprocess.run(['cs2cs', '-f', '%.10f', 'EPSG:3006', 'EPSG:4326'], input=f'{north} {east}\n', capture_output=True, text=True).stdout.split()
    return float(out[1]), float(out[0])


def ecef(lon, lat, h):
    e2 = F * (2 - F)
    sl, cl, sp, cp = math.sin(math.radians(lon)), math.cos(math.radians(lon)), math.sin(math.radians(lat)), math.cos(math.radians(lat))
    n = A / math.sqrt(1 - e2 * sp * sp)
    return [(n + h) * cp * cl, (n + h) * cp * sl, (n * (1 - e2) + h) * sp]


def frame(lon, lat, h, turn):
    """East, north and up at a place, turned clockwise, as a column-major 4 × 4 matrix."""
    sl, cl, sp, cp = math.sin(math.radians(lon)), math.cos(math.radians(lon)), math.sin(math.radians(lat)), math.cos(math.radians(lat))
    e, n, u = [-sl, cl, 0], [-sp * cl, -sp * sl, cp], [cp * cl, cp * sl, sp]
    t = math.radians(turn)
    x = [math.cos(t) * e[k] - math.sin(t) * n[k] for k in range(3)]
    y = [math.sin(t) * e[k] + math.cos(t) * n[k] for k in range(3)]
    o = ecef(lon, lat, h)
    return x + [0] + y + [0] + u + [0] + o + [1]


def pad(b, to=8, fill=b' '):
    return b + fill * ((-len(b)) % to)


def b3dm(glb, name, built):
    ft = pad(json.dumps({'BATCH_LENGTH': 1}).encode())
    bt = pad(json.dumps({'name': [name], 'built': [built]}).encode())
    body = ft + bt + pad(glb, 8, b'\0')
    header = b'b3dm' + struct.pack('<6I', 1, 28 + len(body), len(ft), 0, len(bt), 0)
    return header + body


os.makedirs(OUT, exist_ok=True)
glb = open(os.path.join(ROOT, 'examples', 'house', 'house.glb'), 'rb').read()
children, lons, lats = [], [], []
for i, (name, built, east, north, ground, turn) in enumerate(HOUSES):
    lon, lat = lonlat(east, north)
    lons.append(lon)
    lats.append(lat)
    file = f'house_{i + 1}.b3dm'
    open(os.path.join(OUT, file), 'wb').write(b3dm(glb, name, built))
    children.append({
        'transform': frame(lon, lat, ground + GEOID, turn),
        'boundingVolume': {'box': [0, 0, 4, 8, 0, 0, 0, 8, 0, 0, 0, 5]},
        'geometricError': 0,
        'content': {'uri': file},
    })
pad_deg = 0.0005
region = [math.radians(min(lons) - pad_deg), math.radians(min(lats) - pad_deg), math.radians(max(lons) + pad_deg), math.radians(max(lats) + pad_deg), 100, 140]
tileset = {
    'asset': {'version': '1.0', 'generator': 'Ographis tools/make_tiles_example.py'},
    'geometricError': 100,
    'root': {'boundingVolume': {'region': region}, 'geometricError': 20, 'refine': 'ADD', 'children': children},
}
open(os.path.join(OUT, 'tileset.json'), 'w').write(json.dumps(tileset, indent=1) + '\n')
print('wrote', OUT)
