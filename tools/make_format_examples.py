#!/usr/bin/env python3
"""Writes the example files in examples/formats: a CAD drawing (plan.dxf), the towns as
GeoParquet (towns.parquet), and the made-up blocks of examples/blocks.geojson as a city
model in CityJSON (blocks.city.json) and CityGML 2.0 (blocks.gml), with LoD1 and, for the
rectangular ones, LoD2 with a gable roof.

Needs ezdxf and pyarrow:  pip install ezdxf pyarrow
    python3 tools/make_format_examples.py
"""
import csv
import json
import math
import os
import struct

root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples")
out = os.path.join(root, "formats")
os.makedirs(out, exist_ok=True)

# ---------- DXF ----------
import ezdxf

doc = ezdxf.new("R2018", setup=True)
doc.header["$INSUNITS"] = 6
doc.layers.add("BYGGNAD", color=1)
doc.layers.add("VAG", color=3, linetype="DASHED")
doc.layers.add("VA_BRUNN", color=5)
doc.layers.add("TEXT", color=7)
msp = doc.modelspace()
blocks = json.load(open(os.path.join(root, "blocks.geojson"), encoding="utf-8"))
for f in blocks["features"]:
    ring = f["geometry"]["coordinates"][0]
    msp.add_lwpolyline([tuple(p) for p in ring[:-1]], close=True, dxfattribs={"layer": "BYGGNAD"})
    xs = [p[0] for p in ring]
    ys = [p[1] for p in ring]
    t = msp.add_text(f["properties"]["id"], dxfattribs={"layer": "TEXT", "height": 2.0})
    t.set_placement((sum(xs) / len(xs) - 2, sum(ys) / len(ys) - 1))
a = math.radians(20)
for row in (15, 45):
    x0, y0 = 418230 - 30 * math.cos(a) - row * math.sin(a), 6472520 - 30 * math.sin(a) + row * math.cos(a)
    x1, y1 = x0 + 160 * math.cos(a), y0 + 160 * math.sin(a)
    msp.add_line((x0, y0), (x1, y1), dxfattribs={"layer": "VAG"})
well = doc.blocks.new("BRUNN")
well.add_circle((0, 0), 0.6)
well.add_attdef("ID", (0.8, 0), dxfattribs={"height": 0.4})
well.add_attdef("DJUP", (0.8, -0.5), dxfattribs={"height": 0.4})
for k in range(6):
    x = 418200 + k * 25 * math.cos(a) - 30 * math.sin(a)
    y = 6472520 + k * 25 * math.sin(a) + 30 * math.cos(a)
    ref = msp.add_blockref("BRUNN", (x, y), dxfattribs={"layer": "VA_BRUNN"})
    ref.add_auto_attribs({"ID": f"B-{k + 1:02d}", "DJUP": f"{1.8 + 0.3 * k:.1f}"})
doc.saveas(os.path.join(out, "plan.dxf"))

# ---------- GeoParquet ----------
import pyarrow as pa
import pyarrow.parquet as pq

names, munis, pops, wkb = [], [], [], []
with open(os.path.join(root, "towns.csv"), encoding="utf-8") as f:
    for r in csv.DictReader(f, delimiter=";"):
        lat, lon = float(r["lat"].replace(",", ".")), float(r["lon"].replace(",", "."))
        names.append(r["name"])
        munis.append(r["municipality"])
        pops.append(int(r["population"]))
        wkb.append(struct.pack("<BIdd", 1, 1, lon, lat))
table = pa.table({"name": names, "municipality": munis, "population": pa.array(pops, type=pa.int64()), "geometry": pa.array(wkb, type=pa.binary())})
geo = {"version": "1.1.0", "primary_column": "geometry", "columns": {"geometry": {"encoding": "WKB", "geometry_types": ["Point"]}}}
pq.write_table(table.replace_schema_metadata({b"geo": json.dumps(geo).encode()}), os.path.join(out, "towns.parquet"), compression="zstd")

# ---------- City model ----------
ground = 84.0
objects = {}
vertices = []


def vid(p):
    vertices.append([round(p[0], 3), round(p[1], 3), round(p[2], 3)])
    return len(vertices) - 1


gml_members = []


def poslist(ring):
    return " ".join(f"{p[0]:.3f} {p[1]:.3f} {p[2]:.3f}" for p in ring + [ring[0]])


def gml_poly(ring):
    return f"<gml:Polygon><gml:exterior><gml:LinearRing><gml:posList>{poslist(ring)}</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon>"


for f in blocks["features"]:
    ring = [tuple(p) for p in f["geometry"]["coordinates"][0][:-1]]
    # Counterclockwise seen from above.
    area = sum(ring[i][0] * ring[(i + 1) % len(ring)][1] - ring[(i + 1) % len(ring)][0] * ring[i][1] for i in range(len(ring)))
    if area < 0:
        ring.reverse()
    floors = f["properties"]["floors"]
    h = floors * 3.0
    bid = f["properties"]["id"]
    # LoD1: a block.
    bottom = [(x, y, ground) for x, y in ring]
    top = [(x, y, ground + h) for x, y in ring]
    lod1_faces = [list(reversed(bottom)), top] + [[bottom[i], bottom[(i + 1) % len(ring)], top[(i + 1) % len(ring)], top[i]] for i in range(len(ring))]
    geoms = [{"type": "Solid", "lod": "1", "boundaries": [[[[vid(p) for p in face]] for face in lod1_faces]],
              "semantics": {"surfaces": [{"type": "GroundSurface"}, {"type": "RoofSurface"}, {"type": "WallSurface"}],
                            "values": [[0, 1] + [2] * len(ring)]}}]
    gml_lod1 = "".join(f"<gml:surfaceMember>{gml_poly(face)}</gml:surfaceMember>" for face in lod1_faces)
    gml_lod2 = ""
    # LoD2 for the rectangles: a gable roof along the longer side, 2.5 m high.
    if len(ring) == 4:
        e = [math.dist(ring[i], ring[(i + 1) % 4]) for i in range(4)]
        k = 0 if e[0] >= e[1] else 1
        p = [ring[(k + i) % 4] for i in range(4)]
        mid_a = ((p[0][0] + p[3][0]) / 2, (p[0][1] + p[3][1]) / 2)
        mid_b = ((p[1][0] + p[2][0]) / 2, (p[1][1] + p[2][1]) / 2)
        eave, ridge = ground + h - 2.5, ground + h
        b = [(x, y, ground) for x, y in p]
        t = [(x, y, eave) for x, y in p]
        ra, rb = (mid_a[0], mid_a[1], ridge), (mid_b[0], mid_b[1], ridge)
        faces = [
            (list(reversed(b)), 0),
            ([t[0], t[1], rb, ra], 1),
            ([t[2], t[3], ra, rb], 1),
            ([b[0], b[1], t[1], t[0]], 2),
            ([b[1], b[2], t[2], rb, t[1]], 2),
            ([b[2], b[3], t[3], t[2]], 2),
            ([b[3], b[0], t[0], ra, t[3]], 2),
        ]
        geoms.append({"type": "Solid", "lod": "2", "boundaries": [[[[vid(q) for q in face]] for face, _ in faces]],
                      "semantics": {"surfaces": [{"type": "GroundSurface"}, {"type": "RoofSurface"}, {"type": "WallSurface"}],
                                    "values": [[s for _, s in faces]]}})
        kinds = {0: "GroundSurface", 1: "RoofSurface", 2: "WallSurface"}
        gml_lod2 = "".join(
            f"<bldg:boundedBy><bldg:{kinds[s]}><bldg:lod2MultiSurface><gml:MultiSurface><gml:surfaceMember>{gml_poly(face)}</gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:{kinds[s]}></bldg:boundedBy>"
            for face, s in faces)
    objects[bid] = {"type": "Building", "attributes": {"storeysAboveGround": floors, "measuredHeight": h, "function": "bostad" if floors < 6 else "kontor"}, "geometry": geoms}
    gml_members.append(
        f'<core:cityObjectMember><bldg:Building gml:id="{bid}"><gml:name>{bid}</gml:name>'
        f'<gen:stringAttribute name="objekttyp"><gen:value>Bostadshus</gen:value></gen:stringAttribute>'
        f'<bldg:function>{"bostad" if floors < 6 else "kontor"}</bldg:function><bldg:measuredHeight uom="m">{h}</bldg:measuredHeight>'
        f'<bldg:storeysAboveGround>{floors}</bldg:storeysAboveGround>'
        f'<bldg:lod1Solid><gml:Solid><gml:exterior><gml:CompositeSurface>{gml_lod1}</gml:CompositeSurface></gml:exterior></gml:Solid></bldg:lod1Solid>'
        f'{gml_lod2}</bldg:Building></core:cityObjectMember>')

# CityJSON keeps whole numbers and a transform.
scale = 0.001
tx = [min(v[i] for v in vertices) for i in range(3)]
ints = [[round((v[i] - tx[i]) / scale) for i in range(3)] for v in vertices]
city = {"type": "CityJSON", "version": "2.0", "transform": {"scale": [scale] * 3, "translate": tx},
        "metadata": {"referenceSystem": "https://www.opengis.net/def/crs/EPSG/0/3006", "title": "Made-up blocks for Ographis"},
        "CityObjects": objects, "vertices": ints}
with open(os.path.join(out, "blocks.city.json"), "w", encoding="utf-8") as f:
    json.dump(city, f, ensure_ascii=False, separators=(",", ":"))
with open(os.path.join(out, "blocks.gml"), "w", encoding="utf-8") as f:
    f.write('<?xml version="1.0" encoding="UTF-8"?>\n<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0" '
            'xmlns:bldg="http://www.opengis.net/citygml/building/2.0" xmlns:gen="http://www.opengis.net/citygml/generics/2.0" '
            'xmlns:gml="http://www.opengis.net/gml">\n'
            '<gml:boundedBy><gml:Envelope srsName="EPSG:3006" srsDimension="3"/></gml:boundedBy>\n')
    f.write("\n".join(gml_members))
    f.write("\n</core:CityModel>\n")
print("written to", out)
