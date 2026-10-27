#!/usr/bin/env python3
"""Writes examples/house/villa.ifc: a made-up one-storey house in IFC4, with walls that
have openings for windows and a door, the windows and the door in them, a floor slab, a
gable roof of two slabs, property sets and quantities, styles, and a map conversion that
puts it on the made-up terrain in SWEREF 99 TM (EPSG:3006).

    python3 tools/make_ifc_example.py
"""
import math
import os

lines = []
n = [0]


def e(text):
    n[0] += 1
    lines.append(f"#{n[0]}={text};")
    return f"#{n[0]}"


def guid():
    chars = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$"
    k = n[0] * 7919 + 104729
    s = ""
    for _ in range(22):
        s += chars[k % 64]
        k = (k * 1103515245 + 12345) % (2**31)
    return s


def pt(x, y, z=None):
    return e(f"IFCCARTESIANPOINT(({x:.1f},{y:.1f}))" if z is None else f"IFCCARTESIANPOINT(({x:.1f},{y:.1f},{z:.1f}))")


def d3(x, y, z):
    return e(f"IFCDIRECTION(({x:.6f},{y:.6f},{z:.6f}))")


def place3(x, y, z, axis=None, ref=None):
    return e(f"IFCAXIS2PLACEMENT3D({pt(x, y, z)},{axis or '$'},{ref or '$'})")


def local(rel, x, y, z, axis=None, ref=None):
    return e(f"IFCLOCALPLACEMENT({rel or '$'},{place3(x, y, z, axis, ref)})")


def rect(w, h, cx, cy):
    p2 = e(f"IFCAXIS2PLACEMENT2D({pt(cx, cy)},$)")
    return e(f"IFCRECTANGLEPROFILEDEF(.AREA.,$,{p2},{w:.1f},{h:.1f})")


up = None
ctx = None


def body(items):
    rep = e(f"IFCSHAPEREPRESENTATION({ctx},'Body','SweptSolid',({','.join(items)}))")
    return e(f"IFCPRODUCTDEFINITIONSHAPE($,$,({rep}))")


def extrusion(profile, depth, at=None):
    return e(f"IFCEXTRUDEDAREASOLID({profile},{at or place3(0, 0, 0)},{up},{depth:.1f})")


def style(item, r, g, b, transparency=0.0):
    rgb = e(f"IFCCOLOURRGB($,{r},{g},{b})")
    ren = e(f"IFCSURFACESTYLERENDERING({rgb},{transparency},$,$,$,$,$,$,.NOTDEFINED.)")
    st = e(f"IFCSURFACESTYLE($,.BOTH.,({ren}))")
    e(f"IFCSTYLEDITEM({item},({st}),$)")


def pset(objects, name, props):
    ps = []
    for k, v in props.items():
        if isinstance(v, bool):
            val = f"IFCBOOLEAN(.{'T' if v else 'F'}.)"
        elif isinstance(v, (int, float)):
            val = f"IFCREAL({float(v)})"
        else:
            val = f"IFCLABEL('{v}')"
        ps.append(e(f"IFCPROPERTYSINGLEVALUE('{k}',$,{val},$)"))
    s = e(f"IFCPROPERTYSET('{guid()}',$,'{name}',$,({','.join(ps)}))")
    e(f"IFCRELDEFINESBYPROPERTIES('{guid()}',$,$,$,({','.join(objects)}),{s})")


def quantities(obj, name, q):
    qs = []
    for k, (kind, v) in q.items():
        qs.append(e(f"IFCQUANTITY{kind}('{k}',$,$,{v:.3f},$)"))
    s = e(f"IFCELEMENTQUANTITY('{guid()}',$,'{name}',$,$,({','.join(qs)}))")
    e(f"IFCRELDEFINESBYPROPERTIES('{guid()}',$,$,$,({obj}),{s})")


units = e("IFCUNITASSIGNMENT((#{a},#{b}))".format(a=n[0] + 2, b=n[0] + 3))
e("IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.)")
e("IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.)")
up = d3(0, 0, 1)
ctx = e(f"IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,{place3(0, 0, 0)},$)")
project = e(f"IFCPROJECT('{guid()}',$,'Villa',$,$,$,$,({ctx}),{units})")
crs = e("IFCPROJECTEDCRS('EPSG:3006','SWEREF99 TM','SWEREF99',$,$,$,$)")
# Turned 20 degrees counterclockwise from east, standing on the terrain at 84.3 m.
a = math.radians(20)
e(f"IFCMAPCONVERSION({ctx},{crs},418150.,6472440.,84.3,{math.cos(a):.6f},{math.sin(a):.6f},0.001)")

site_pl = local(None, 0, 0, 0)
site = e(f"IFCSITE('{guid()}',$,'Tomt',$,$,{site_pl},$,$,.ELEMENT.,$,$,$,$,$)")
bldg_pl = local(site_pl, 0, 0, 0)
bldg = e(f"IFCBUILDING('{guid()}',$,'Villa Ekbacken',$,$,{bldg_pl},$,$,.ELEMENT.,$,$,$)")
st_pl = local(bldg_pl, 0, 0, 0)
storey = e(f"IFCBUILDINGSTOREY('{guid()}',$,'Plan 1',$,$,{st_pl},$,$,.ELEMENT.,0.)")
e(f"IFCRELAGGREGATES('{guid()}',$,$,$,{project},({site}))")
e(f"IFCRELAGGREGATES('{guid()}',$,$,$,{site},({bldg}))")
e(f"IFCRELAGGREGATES('{guid()}',$,$,$,{bldg},({storey}))")

L, W, H, T = 11000.0, 7000.0, 2900.0, 300.0
contained = []
walls = []
# Walls: (name, x0, y0, direction, length, external)
specs = [
    ("Yttervägg söder", 0, 0, (1, 0), L),
    ("Yttervägg öster", L, 0, (0, 1), W),
    ("Yttervägg norr", L, W, (-1, 0), L),
    ("Yttervägg väster", 0, W, (0, -1), W),
]
for name, x0, y0, (dx, dy), length in specs:
    pl = local(st_pl, x0, y0, 0, None, d3(dx, dy, 0))
    item = extrusion(rect(length, T, length / 2, T / 2), H)
    style(item, 0.93, 0.91, 0.86)
    w = e(f"IFCWALL('{guid()}',$,'{name}',$,$,{pl},{body([item])},$,.STANDARD.)")
    walls.append((w, pl, length))
    contained.append(w)
    quantities(w, "Qto_WallBaseQuantities", {"Length": ("LENGTH", length / 1000), "Height": ("LENGTH", H / 1000), "Width": ("LENGTH", T / 1000)})
# The gables, above the short walls: a triangle in the plane of the wall.
ridge = (W / 2) * math.tan(math.radians(30))
for name, x0, y0, along, normal in (("Gavel öster", L, 0, (0, 1, 0), (-1, 0, 0)), ("Gavel väster", 0, W, (0, -1, 0), (1, 0, 0))):
    pl = local(st_pl, x0, y0, 0, d3(*normal), d3(*along))
    poly = e(f"IFCPOLYLINE(({pt(0, -H)},{pt(W, -H)},{pt(W / 2, -(H + ridge))},{pt(0, -H)}))")
    prof = e(f"IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,{poly})")
    item = extrusion(prof, T)
    style(item, 0.93, 0.91, 0.86)
    g = e(f"IFCWALL('{guid()}',$,'{name}',$,$,{pl},{body([item])},$,.STANDARD.)")
    contained.append(g)
pset([w for w, _, _ in walls], "Pset_WallCommon", {"IsExternal": True, "LoadBearing": True, "FireRating": "EI 30"})

# Windows and a door, in openings through the walls.
windows = []
for wi, (w, pl, length) in enumerate(walls):
    places = [length * f for f in ((0.2, 0.5, 0.8) if length > 8000 else (0.5,))]
    for k, cx in enumerate(places):
        is_door = wi == 0 and k == 1
        ww, wh, sill = (1000.0, 2100.0, 0.0) if is_door else (1300.0, 1300.0, 900.0)
        op_pl = local(pl, cx - ww / 2, -50, sill)
        op_item = extrusion(rect(ww, T + 100, ww / 2, (T + 100) / 2), wh)
        op = e(f"IFCOPENINGELEMENT('{guid()}',$,'Öppning',$,$,{op_pl},{body([op_item])},$,.OPENING.)")
        e(f"IFCRELVOIDSELEMENT('{guid()}',$,$,$,{w},{op})")
        fill_pl = local(op_pl, 0, 50 + T / 2 - 40, 0)
        if is_door:
            leaf = extrusion(rect(ww, 60, ww / 2, 30), wh)
            style(leaf, 0.45, 0.30, 0.20)
            el = e(f"IFCDOOR('{guid()}',$,'Entrédörr',$,$,{fill_pl},{body([leaf])},$,{wh:.1f},{ww:.1f},.DOOR.,.SINGLE_SWING_LEFT.,$)")
        else:
            frame_out = rect(ww, 80, ww / 2, 40)
            frame = extrusion(frame_out, 80)
            top = extrusion(frame_out, 80, place3(0, 0, wh - 80))
            left = extrusion(rect(80, 80, 40, 40), wh)
            right = extrusion(rect(80, 80, ww - 40, 40), wh)
            glass = extrusion(rect(ww - 160, 20, ww / 2, 40), wh - 160, place3(0, 0, 80))
            for f in (frame, top, left, right):
                style(f, 0.97, 0.97, 0.95)
            style(glass, 0.6, 0.72, 0.8, 0.7)
            el = e(f"IFCWINDOW('{guid()}',$,'Fönster {len(windows) + 1}',$,$,{fill_pl},{body([frame, top, left, right, glass])},$,{wh:.1f},{ww:.1f},.WINDOW.,.SINGLE_PANEL.,$)")
            windows.append(el)
        e(f"IFCRELFILLSELEMENT('{guid()}',$,$,$,{op},{el})")
        contained.append(el)
pset(windows, "Pset_WindowCommon", {"IsExternal": True, "ThermalTransmittance": 0.9})

# The floor slab, and the roof of two slabs that slope 30 degrees from the long walls.
slab_item = extrusion(rect(L, W, L / 2, W / 2), 250, place3(0, 0, -250))
style(slab_item, 0.7, 0.7, 0.68)
slab = e(f"IFCSLAB('{guid()}',$,'Bottenplatta',$,$,{local(st_pl, 0, 0, 0)},{body([slab_item])},$,.FLOOR.)")
contained.append(slab)
pitch = math.radians(30)
eave = 500.0
run = W / 2 + eave
slope = run / math.cos(pitch)
roof_parts = []
for side in (0, 1):
    # The slab lies in a plane tilted about the long axis: its local y goes up the slope.
    if side == 0:
        origin = (-eave, -eave, H - eave * math.tan(pitch))
        ydir = (0, math.cos(pitch), math.sin(pitch))
    else:
        origin = (L + eave, W + eave, H - eave * math.tan(pitch))
        ydir = (0, -math.cos(pitch), math.sin(pitch))
    xdir = (1, 0, 0) if side == 0 else (-1, 0, 0)
    z = (xdir[1] * ydir[2] - xdir[2] * ydir[1], xdir[2] * ydir[0] - xdir[0] * ydir[2], xdir[0] * ydir[1] - xdir[1] * ydir[0])
    pl = local(st_pl, *origin, d3(*z), d3(*xdir))
    item = extrusion(rect(L + 2 * eave, slope, (L + 2 * eave) / 2, slope / 2), 200)
    style(item, 0.42, 0.20, 0.16)
    roof_parts.append(e(f"IFCSLAB('{guid()}',$,'Takfall {'söder' if side == 0 else 'norr'}',$,$,{pl},{body([item])},$,.ROOF.)"))
roof = e(f"IFCROOF('{guid()}',$,'Tak',$,$,{local(st_pl, 0, 0, 0)},$,$,.GABLE_ROOF.)")
e(f"IFCRELAGGREGATES('{guid()}',$,$,$,{roof},({','.join(roof_parts)}))")
contained.append(roof)
pset(roof_parts, "Pset_SlabCommon", {"IsExternal": True})
e(f"IFCRELCONTAINEDINSPATIALSTRUCTURE('{guid()}',$,$,$,({','.join(contained)}),{storey})")

out = os.path.join(os.path.dirname(__file__), "..", "examples", "house", "villa.ifc")
with open(out, "w", encoding="ascii") as f:
    f.write("ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ViewDefinition [ReferenceView_V1.2]'),'2;1');\n")
    f.write("FILE_NAME('villa.ifc','2026-10-22T12:00:00',('Ographis'),('Ographis'),'','tools/make_ifc_example.py','');\n")
    f.write("FILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n")
    for line in lines:
        # Letters beyond ASCII as STEP writes them.
        f.write("".join(c if ord(c) < 128 else "\\X2\\%04X\\X0\\" % ord(c) for c in line) + "\n")
    f.write("ENDSEC;\nEND-ISO-10303-21;\n")
print(out, len(lines), "records")
