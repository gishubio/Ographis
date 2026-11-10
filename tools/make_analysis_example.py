"""Writes examples/analysis: a made-up small town on the made-up terrain, for the analysis
tools of 0.96. All in SWEREF 99 TM (EPSG:3006), and nothing in it is real.

- roads.geojson: a grid of streets 200 m apart with a ring road and two roads out of town,
  each with a speed limit in km/h.
- people.geojson: squares of 250 m with the number of people in each, most in the middle and
  in a suburb to the north-east.
- districts.geojson: three districts that do not follow the squares.
- health_centres.geojson: the two health centres there are.
- candidates.geojson: eight places where a new one could be.
- walk.csv: a walk with a GPS, a point every 20 seconds, with a stop for coffee and a pause
  where the GPS lost its signal, and one point that jumps away.
- wind.csv: wind measured at twelve places, the direction it comes from and its speed.
"""
import json, math, os, random

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, 'examples', 'analysis')
X0, Y0 = 414000.0, 6472000.0  # the lower left corner of the town
SIZE = 4000.0
CRS = {"type": "name", "properties": {"name": "urn:ogc:def:crs:EPSG::3006"}}
random.seed(96)


def write(name, features):
    with open(os.path.join(OUT, name), 'w', encoding='utf-8') as f:
        json.dump({"type": "FeatureCollection", "crs": CRS, "features": features}, f, ensure_ascii=False)
        f.write('\n')


def feature(props, geom):
    return {"type": "Feature", "properties": props, "geometry": geom}


def r1(v):
    return round(v, 1)


def roads():
    out = []
    for k in range(0, 21):
        v = k * 200.0
        # The streets of the town, 40 km/h, and the main streets through the middle, 60.
        speed = 60 if k == 10 else 40
        out.append(feature({"name": f"Street {k + 1}", "speed": speed}, {"type": "LineString", "coordinates": [[X0, Y0 + v], [X0 + SIZE, Y0 + v]]}))
        out.append(feature({"name": f"Avenue {k + 1}", "speed": speed}, {"type": "LineString", "coordinates": [[X0 + v, Y0], [X0 + v, Y0 + SIZE]]}))
    # A ring road of 70 km/h around the middle, and two roads out of town of 90.
    ring = [[r1(X0 + 2000 + 1500 * math.cos(a / 24 * 2 * math.pi)), r1(Y0 + 2000 + 1500 * math.sin(a / 24 * 2 * math.pi))] for a in range(25)]
    out.append(feature({"name": "Ring road", "speed": 70}, {"type": "LineString", "coordinates": ring}))
    out.append(feature({"name": "Road west", "speed": 90}, {"type": "LineString", "coordinates": [[X0 - 2000, Y0 + 2000], [X0, Y0 + 2000]]}))
    out.append(feature({"name": "Road north-east", "speed": 90}, {"type": "LineString", "coordinates": [[X0 + SIZE, Y0 + SIZE], [X0 + SIZE + 1500, Y0 + SIZE + 1500]]}))
    write('roads.geojson', out)


def people():
    out = []
    for r in range(16):
        for c in range(16):
            x, y = X0 + c * 250.0, Y0 + r * 250.0
            mx, my = x + 125 - (X0 + 2000), y + 125 - (Y0 + 2000)
            centre = 900 * math.exp(-(mx * mx + my * my) / (2 * 700.0 ** 2))
            sx, sy = x + 125 - (X0 + 3300), y + 125 - (Y0 + 3300)
            suburb = 500 * math.exp(-(sx * sx + sy * sy) / (2 * 350.0 ** 2))
            n = int(centre + suburb + random.uniform(0, 40))
            if n < 15:
                continue
            ring = [[x, y], [x + 250, y], [x + 250, y + 250], [x, y + 250], [x, y]]
            out.append(feature({"people": n}, {"type": "Polygon", "coordinates": [ring]}))
    write('people.geojson', out)


def districts():
    # Three districts that meet at a point off the middle, so that they cut through squares.
    cx, cy = X0 + 1870.0, Y0 + 2130.0
    corners = {"West": [[X0, Y0], [cx, Y0], [cx, cy], [X0, cy + 300]],
               "North": [[X0, cy + 300], [cx, cy], [X0 + SIZE, cy + 200], [X0 + SIZE, Y0 + SIZE], [X0, Y0 + SIZE]],
               "East": [[cx, cy], [cx, Y0], [X0 + SIZE, Y0], [X0 + SIZE, cy + 200]]}
    out = []
    for name, pts in corners.items():
        ring = pts + [pts[0]]
        out.append(feature({"district": name}, {"type": "Polygon", "coordinates": [ring]}))
    write('districts.geojson', out)


def places():
    write('health_centres.geojson', [
        feature({"name": "Centre health centre", "there": "yes"}, {"type": "Point", "coordinates": [X0 + 1980.0, Y0 + 2010.0]}),
        feature({"name": "West health centre", "there": "yes"}, {"type": "Point", "coordinates": [X0 + 610.0, Y0 + 1190.0]}),
    ])
    sites = [("Old school", 3410, 3390), ("Station", 2010, 3590), ("Mill", 3610, 990), ("Square", 1210, 2790),
             ("Harbour", 390, 3610), ("Park", 2810, 2410), ("Field", 3390, 2810), ("Church", 2590, 1410)]
    write('candidates.geojson', [feature({"name": n}, {"type": "Point", "coordinates": [X0 + x, Y0 + y]}) for n, x, y in sites])


def walk():
    rows = ["time;note;e;n"]
    t = 9 * 3600  # 09:00
    x, y = X0 + 400.0, Y0 + 400.0

    def at(t, x, y, note=""):
        h, m, s = t // 3600, (t % 3600) // 60, t % 60
        rows.append(f"2026-06-14T{h:02d}:{m:02d}:{s:02d}Z;{note};{r1(x)};{r1(y)}")

    # North along an avenue at 5 km/h (28 m in 20 s), then east.
    for _ in range(60):
        at(t, x + random.uniform(-3, 3), y + random.uniform(-3, 3))
        y += 28
        t += 20
    for _ in range(30):
        at(t, x + random.uniform(-3, 3), y + random.uniform(-3, 3))
        x += 28
        t += 20
    # Coffee: 15 minutes within a few metres.
    for _ in range(45):
        at(t, x + random.uniform(-6, 6), y + random.uniform(-6, 6), "coffee")
        t += 20
    # One point the GPS put far away.
    at(t, x + 2500, y + 2500, "jump")
    t += 20
    for _ in range(30):
        at(t, x + random.uniform(-3, 3), y + random.uniform(-3, 3))
        x += 28
        t += 20
    # The signal is lost for 40 minutes; then home by bike at 15 km/h.
    t += 40 * 60
    for _ in range(40):
        at(t, x + random.uniform(-3, 3), y + random.uniform(-3, 3))
        y -= 83
        t += 20
    with open(os.path.join(OUT, 'walk.csv'), 'w', encoding='utf-8') as f:
        f.write("\n".join(rows) + "\n")


def wind():
    rows = ["station;from_degrees;speed;e;n"]
    for k in range(12):
        x = X0 - 1500 + (k % 4) * 2300.0
        y = Y0 - 500 + (k // 4) * 2400.0
        rows.append(f"W{k + 1};{round(250 + random.uniform(-25, 25))};{round(random.uniform(3, 11), 1)};{x};{y}")
    with open(os.path.join(OUT, 'wind.csv'), 'w', encoding='utf-8') as f:
        f.write("\n".join(rows) + "\n")


if __name__ == '__main__':
    os.makedirs(OUT, exist_ok=True)
    roads()
    people()
    districts()
    places()
    walk()
    wind()
    print('Written to', OUT)
