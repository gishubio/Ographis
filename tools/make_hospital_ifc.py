#!/usr/bin/env python3
"""Writes examples/hospital/hospital.ifc: a made-up hospital building in IFC4 to try the
reading of IFC with, and to show. A ground floor and four floors of wards on a grid of
columns, with exterior walls of brick, walls between the rooms, doors to the corridor,
windows in openings, rooms as spaces with their departments, a stair, two lifts, beds,
floor slabs, a flat roof with a parapet, property sets and quantities, materials and
colours, and a map conversion that puts it in SWEREF 99 TM (EPSG:3006) in Göteborg.

    python3 tools/make_hospital_ifc.py
"""
import math
import os
import uuid

lines = []
n = [0]


def e(text):
    n[0] += 1
    lines.append(f"#{n[0]}={text};")
    return f"#{n[0]}"


CHARS = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$"
made = [0]


def guid():
    """A GlobalId of 22 characters, the same in every run of the script."""
    made[0] += 1
    v = uuid.uuid5(uuid.NAMESPACE_URL, f"ographis-hospital-{made[0]}").int
    s = CHARS[v >> 126]
    for i in range(21):
        s += CHARS[(v >> (120 - 6 * i)) & 63]
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


def body(items):
    rep = e(f"IFCSHAPEREPRESENTATION({ctx},'Body','SweptSolid',({','.join(items)}))")
    return e(f"IFCPRODUCTDEFINITIONSHAPE($,$,({rep}))")


def extrusion(profile, depth, at=None):
    return e(f"IFCEXTRUDEDAREASOLID({profile},{at or place3(0, 0, 0)},{up},{depth:.1f})")


def box(x, y, z, w, d, h):
    """A box from x, y, z, w long in x, d deep in y and h high."""
    return extrusion(rect(w, d, x + w / 2, y + d / 2), h, place3(0, 0, z))


styles = {}


def style(item, colour, transparency=0.0):
    key = (colour, transparency)
    if key not in styles:
        rgb = e(f"IFCCOLOURRGB($,{colour[0]},{colour[1]},{colour[2]})")
        ren = e(f"IFCSURFACESTYLERENDERING({rgb},{transparency},$,$,$,$,$,$,.NOTDEFINED.)")
        styles[key] = e(f"IFCSURFACESTYLE($,.BOTH.,({ren}))")
    e(f"IFCSTYLEDITEM({item},({styles[key]}),$)")
    return item


def text(v):
    return v.replace("'", "''")


def pset(objects, name, props):
    if not objects:
        return
    ps = []
    for k, v in props.items():
        if isinstance(v, bool):
            val = f"IFCBOOLEAN(.{'T' if v else 'F'}.)"
        elif isinstance(v, int):
            val = f"IFCINTEGER({v})"
        elif isinstance(v, float):
            val = f"IFCREAL({v})"
        else:
            val = f"IFCLABEL('{text(v)}')"
        ps.append(e(f"IFCPROPERTYSINGLEVALUE('{k}',$,{val},$)"))
    s = e(f"IFCPROPERTYSET('{guid()}',$,'{name}',$,({','.join(ps)}))")
    e(f"IFCRELDEFINESBYPROPERTIES('{guid()}',$,$,$,({','.join(objects)}),{s})")


def quantities(obj, name, q):
    qs = [e(f"IFCQUANTITY{kind}('{k}',$,$,{v:.3f},$)") for k, (kind, v) in q.items()]
    s = e(f"IFCELEMENTQUANTITY('{guid()}',$,'{name}',$,$,({','.join(qs)}))")
    e(f"IFCRELDEFINESBYPROPERTIES('{guid()}',$,$,$,({obj}),{s})")


materials = {}
material_of = {}


def material(obj, name):
    if name not in materials:
        materials[name] = e(f"IFCMATERIAL('{name}',$,$)")
    material_of.setdefault(name, []).append(obj)


units = e("IFCUNITASSIGNMENT((#{a},#{b},#{c}))".format(a=n[0] + 2, b=n[0] + 3, c=n[0] + 4))
e("IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.)")
e("IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.)")
e("IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.)")
up = d3(0, 0, 1)
ctx = e(f"IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,{place3(0, 0, 0)},$)")
project = e(f"IFCPROJECT('{guid()}',$,'Hospital',$,$,$,$,({ctx}),{units})")
crs = e("IFCPROJECTEDCRS('EPSG:3006','SWEREF99 TM','SWEREF99',$,$,$,$)")
# In Göteborg, turned 25 degrees counterclockwise from east, the ground floor at 24.5 m.
a = math.radians(25)
e(f"IFCMAPCONVERSION({ctx},{crs},318880.,6397530.,24.5,{math.cos(a):.6f},{math.sin(a):.6f},0.001)")

site_pl = local(None, 0, 0, 0)
site = e(f"IFCSITE('{guid()}',$,'Hospital site',$,$,{site_pl},$,$,.ELEMENT.,$,$,$,$,$)")
bldg_pl = local(site_pl, 0, 0, 0)
bldg = e(f"IFCBUILDING('{guid()}',$,'Building 5, wards',$,$,{bldg_pl},$,$,.ELEMENT.,$,$,$)")
e(f"IFCRELAGGREGATES('{guid()}',$,$,$,{project},({site}))")
e(f"IFCRELAGGREGATES('{guid()}',$,$,$,{site},({bldg}))")

# The building: 18 modules of 3.6 m along x, rooms on both sides of a corridor.
M = 3600.0
L, W = 18 * M, 19200.0
T, TI = 300.0, 120.0  # exterior and interior walls
SLAB = 300.0
CORR_S, CORR_N = 8400.0, 10800.0  # the corridor between these
CORE = (8 * M, 10 * M)  # stair and lifts on the north side
levels = [("Ground floor", 0.0, 4500.0)] + [(f"Level {i}", 4500.0 + (i - 1) * 3900.0, 3900.0) for i in range(1, 5)]

BRICK = (0.66, 0.36, 0.28)
WHITE = (0.94, 0.94, 0.92)
CONCRETE = (0.68, 0.68, 0.66)
FLOOR = (0.78, 0.77, 0.74)
WOOD = (0.62, 0.47, 0.32)
FRAME = (0.80, 0.82, 0.84)
GLASS = (0.55, 0.70, 0.80)
ROOF = (0.32, 0.33, 0.35)
STEEL = (0.55, 0.57, 0.60)
BED = (0.90, 0.92, 0.95)

storeys = []
all_ext, all_int, all_windows, all_doors, all_spaces, all_columns = [], [], [], [], [], []


def wall(st_pl, contained, name, x0, y0, dx, dy, length, thick, height, colour, external, mat):
    """A wall from x0, y0 along dx, dy; its thickness lies to the left of that direction."""
    pl = local(st_pl, x0, y0, 0, None, d3(dx, dy, 0))
    item = style(extrusion(rect(length, thick, length / 2, thick / 2), height), colour)
    w = e(f"IFCWALL('{guid()}',$,'{text(name)}',$,$,{pl},{body([item])},$,.{'STANDARD' if external else 'PARTITIONING'}.)")
    contained.append(w)
    (all_ext if external else all_int).append(w)
    material(w, mat)
    quantities(w, "Qto_WallBaseQuantities", {"Length": ("LENGTH", length / 1000), "Height": ("LENGTH", height / 1000), "Width": ("LENGTH", thick / 1000)})
    return w, pl


def opening(w, wpl, thick, at, width, height, sill):
    op_pl = local(wpl, at - width / 2, -50, sill)
    op_item = extrusion(rect(width, thick + 100, width / 2, (thick + 100) / 2), height)
    op = e(f"IFCOPENINGELEMENT('{guid()}',$,'Opening',$,$,{op_pl},{body([op_item])},$,.OPENING.)")
    e(f"IFCRELVOIDSELEMENT('{guid()}',$,$,$,{w},{op})")
    return op, op_pl


def window(contained, w, wpl, thick, at, width, height, sill, name):
    op, op_pl = opening(w, wpl, thick, at, width, height, sill)
    fill_pl = local(op_pl, 0, 50 + thick / 2 - 40, 0)
    parts = [
        style(box(0, 0, 0, width, 80, 80), FRAME),
        style(box(0, 0, height - 80, width, 80, 80), FRAME),
        style(box(0, 0, 0, 80, 80, height), FRAME),
        style(box(width - 80, 0, 0, 80, 80, height), FRAME),
        style(box(width / 2 - 30, 0, 0, 60, 80, height), FRAME),
        style(box(80, 30, 80, width - 160, 20, height - 160), GLASS, 0.6),
    ]
    el = e(f"IFCWINDOW('{guid()}',$,'{text(name)}',$,$,{fill_pl},{body(parts)},$,{height:.1f},{width:.1f},.WINDOW.,.DOUBLE_PANEL_VERTICAL.,$)")
    e(f"IFCRELFILLSELEMENT('{guid()}',$,$,$,{op},{el})")
    contained.append(el)
    all_windows.append(el)
    material(el, "Aluminium and glass")


def door(contained, w, wpl, thick, at, width, height, name, glass=False):
    op, op_pl = opening(w, wpl, thick, at, width, height, 0)
    fill_pl = local(op_pl, 0, 50 + thick / 2 - 25, 0)
    if glass:
        parts = [style(box(0, 0, 0, width, 50, height), GLASS, 0.5)]
    else:
        parts = [style(box(0, 0, 0, width, 50, height), WOOD)]
    el = e(f"IFCDOOR('{guid()}',$,'{text(name)}',$,$,{fill_pl},{body(parts)},$,{height:.1f},{width:.1f},.DOOR.,.{'DOUBLE_DOOR_SINGLE_SWING' if width > 1500 else 'SINGLE_SWING_LEFT'}.,$)")
    e(f"IFCRELFILLSELEMENT('{guid()}',$,$,$,{op},{el})")
    contained.append(el)
    all_doors.append(el)
    material(el, "Glass" if glass else "Oak")


def space(st_pl, spaces, number, long_name, dept, x0, y0, x1, y1, height):
    pl = local(st_pl, 0, 0, 0)
    item = extrusion(rect(x1 - x0, y1 - y0, (x0 + x1) / 2, (y0 + y1) / 2), height)
    s = e(f"IFCSPACE('{guid()}',$,'{number}',$,$,{pl},{body([item])},'{text(long_name)}',.ELEMENT.,.INTERNAL.,$)")
    spaces.append(s)
    all_spaces.append(s)
    area = (x1 - x0) * (y1 - y0) / 1e6
    quantities(s, "Qto_SpaceBaseQuantities", {"NetFloorArea": ("AREA", area), "Height": ("LENGTH", height / 1000)})
    pset([s], "Hospital", {"Department": dept, "Function": long_name})
    return s


def rooms_of(level):
    """The rooms on each side of the corridor, as (first module, modules, name, department)."""
    if level == 0:
        south = [(0, 2, "Ambulance entrance", "Emergency"), (2, 3, "Emergency room", "Emergency"), (5, 2, "Triage", "Emergency"),
                 (7, 4, "Entrance hall", "Entrance"), (11, 2, "Café", "Entrance"), (13, 3, "X-ray", "Imaging"), (16, 2, "CT", "Imaging")]
        north = [(0, 2, "Staff room", "Emergency"), (2, 2, "Treatment room", "Emergency"), (4, 2, "Treatment room", "Emergency"),
                 (6, 2, "Reception", "Entrance"), (10, 2, "Pharmacy", "Entrance"), (12, 2, "Waiting room", "Imaging"),
                 (14, 2, "MRI", "Imaging"), (16, 2, "Changing rooms", "Staff")]
        return south, north
    ward = ["Medicine", "Surgery", "Orthopaedics", "Geriatrics"][level - 1]
    south = []
    for k in range(18):
        if k in (0, 17):
            south.append((k, 1, "Store", ward))
        elif k == 9:
            south.append((k, 1, "Day room", ward))
        else:
            south.append((k, 1, "Patient room", ward))
    north = [(0, 1, "Utility room", ward), (1, 1, "Patient room", ward), (2, 1, "Patient room", ward), (3, 1, "Patient room", ward),
             (4, 2, "Nurses' station", ward), (6, 1, "Medicine room", ward), (7, 1, "Office", ward),
             (10, 1, "Office", ward), (11, 1, "Patient room", ward), (12, 1, "Patient room", ward), (13, 1, "Patient room", ward),
             (14, 1, "Patient room", ward), (15, 1, "Isolation room", ward), (16, 1, "Patient room", ward), (17, 1, "Cleaning room", ward)]
    return south, north


for li, (lname, elev, height) in enumerate(levels):
    st_pl = local(bldg_pl, 0, 0, elev)
    storey = e(f"IFCBUILDINGSTOREY('{guid()}',$,'{lname}',$,$,{st_pl},$,$,.ELEMENT.,{elev:.1f})")
    storeys.append(storey)
    contained, spaces = [], []
    h = height - SLAB
    tag = "G" if li == 0 else str(li)

    # The floor slab.
    slab_item = style(box(0, 0, -SLAB, L, W, SLAB), FLOOR)
    slab = e(f"IFCSLAB('{guid()}',$,'Floor slab {lname.lower()}',$,$,{local(st_pl, 0, 0, 0)},{body([slab_item])},$,.FLOOR.)")
    contained.append(slab)
    material(slab, "Concrete")
    quantities(slab, "Qto_SlabBaseQuantities", {"GrossArea": ("AREA", L * W / 1e6), "Width": ("LENGTH", SLAB / 1000)})

    # Exterior walls, counterclockwise, with windows on the long sides and at the corridor ends.
    south_w = wall(st_pl, contained, f"Exterior wall south {tag}", 0, 0, 1, 0, L, T, h, BRICK, True, "Brick")
    east_w = wall(st_pl, contained, f"Exterior wall east {tag}", L, 0, 0, 1, W, T, h, BRICK, True, "Brick")
    north_w = wall(st_pl, contained, f"Exterior wall north {tag}", L, W, -1, 0, L, T, h, BRICK, True, "Brick")
    west_w = wall(st_pl, contained, f"Exterior wall west {tag}", 0, W, 0, -1, W, T, h, BRICK, True, "Brick")
    ww, wh, sill = (2400.0, 2200.0, 700.0) if li == 0 else (1800.0, 1500.0, 900.0)
    for k in range(18):
        at = k * M + M / 2
        if li == 0 and k in (9, 10):
            continue  # the main entrance
        window(contained, *south_w, T, at, ww, wh, sill, f"Window {tag}.S{k + 1:02d}")
        if not (CORE[0] <= k * M < CORE[1]):
            window(contained, *north_w, T, L - at, ww, wh, sill, f"Window {tag}.N{k + 1:02d}")
    for wall_, name in ((east_w, "E"), (west_w, "W")):
        window(contained, *wall_, T, W / 2, 1800.0, wh, sill, f"Window {tag}.{name}")
    if li == 0:
        door(contained, *south_w, T, 10 * M, 3600.0, 2700.0, "Main entrance", glass=True)
        door(contained, *south_w, T, M, 2400.0, 2700.0, "Ambulance doors", glass=True)

    # The corridor walls, with a door to each room.
    cs = wall(st_pl, contained, f"Corridor wall south {tag}", 0, CORR_S - TI, 1, 0, L, TI, h, WHITE, False, "Gypsum")
    cn = wall(st_pl, contained, f"Corridor wall north {tag}", L, CORR_N + TI, -1, 0, L, TI, h, WHITE, False, "Gypsum")
    south, north = rooms_of(li)
    no = [0]

    def side(rooms, y0, y1, corridor, along_reversed):
        for k, mods, name, dept in rooms:
            x0, x1 = k * M, (k + mods) * M
            no[0] += 1
            number = f"{tag}.{no[0]:02d}"
            space(st_pl, spaces, number, name, dept, x0 + TI / 2, y0, x1 - TI / 2, y1, h)
            if name not in ("Entrance hall",):
                mid = (x0 + x1) / 2
                at = L - mid if along_reversed else mid
                wide = 2400.0 if name in ("Emergency room", "X-ray", "CT", "MRI") else 1200.0
                door(contained, *corridor, TI, at, wide, 2100.0, f"Door {number}")
            # Beds in the patient rooms.
            if name in ("Patient room", "Isolation room"):
                beds = 2 if name == "Patient room" else 1
                for b in range(beds):
                    bx = x0 + 400 + b * 1500
                    by = (y0 + 600) if y0 < CORR_S else (y1 - 600 - 2100)
                    parts = [style(box(0, 0, 0, 1000, 2100, 550), BED), style(box(0, 0 if y0 < CORR_S else 2050, 550, 1000, 50, 400), STEEL)]
                    bed = e(f"IFCFURNITURE('{guid()}',$,'Bed {number}.{b + 1}',$,$,{local(st_pl, bx, by, 0)},{body(parts)},$,.BED.)")
                    contained.append(bed)
        # The walls between the rooms.
        for k, mods, name, dept in rooms[1:]:
            x = k * M
            wall(st_pl, contained, f"Partition {tag} x{int(x)}", x + TI / 2, y0, 0, 1, y1 - y0, TI, h, WHITE, False, "Gypsum")

    side(south, T, CORR_S - TI, cs, False)
    side(north, CORR_N + TI, W - T, cn, True)
    space(st_pl, spaces, f"{tag}.C", "Corridor", "Common", T, CORR_S, L - T, CORR_N, h)

    # The core: a stair and two lifts, walled in concrete.
    cx0, cx1 = CORE
    y0, y1 = CORR_N + TI, W - T
    for x in (cx0, cx0 + M, cx1):
        wall(st_pl, contained, f"Core wall {tag} x{int(x)}", x + 100, y0, 0, 1, y1 - y0, 200, h, CONCRETE, False, "Concrete")
    space(st_pl, spaces, f"{tag}.ST", "Stair", "Common", cx0 + 100, y0, cx0 + M - 100, y1, h)
    space(st_pl, spaces, f"{tag}.LF", "Lifts", "Common", cx0 + M + 100, y0, cx1 - 100, y1, h)
    door(contained, *cn, TI, L - (cx0 + M / 2), 1200.0, 2100.0, f"Stair door {tag}")
    door(contained, *cn, TI, L - (cx0 + M * 1.3), 1100.0, 2100.0, f"Lift door {tag}.1")
    door(contained, *cn, TI, L - (cx0 + M * 1.75), 1100.0, 2100.0, f"Lift door {tag}.2")
    if li < len(levels) - 1:
        # A straight flight up along the stair, to the next floor.
        risers = int(round(height / 175))
        rise = height / risers
        steps = [style(box(0, i * 270, i * rise, 1300, 270, rise), CONCRETE) for i in range(risers)]
        flight = e(f"IFCSTAIRFLIGHT('{guid()}',$,'Flight {tag}',$,$,{local(st_pl, cx0 + 300, y0 + 300, 0)},{body(steps)},$,{risers},{risers - 1},{rise:.1f},270.,.STRAIGHT.)")
        stair = e(f"IFCSTAIR('{guid()}',$,'Stair {tag}',$,$,{local(st_pl, 0, 0, 0)},$,$,.STRAIGHT_RUN_STAIR.)")
        e(f"IFCRELAGGREGATES('{guid()}',$,$,$,{stair},({flight}))")
        contained.append(stair)
    if li == 0:
        for k in range(2):
            car = [style(box(0, 0, 0, 1400, 2100, 2300), STEEL)]
            lift = e(f"IFCTRANSPORTELEMENT('{guid()}',$,'Lift {k + 1}',$,$,{local(st_pl, cx0 + M + 150 + k * 1750, y0 + 600, 0)},{body(car)},$,.ELEVATOR.)")
            contained.append(lift)

    # Columns on the grid, every 7.2 m along both corridor walls.
    for k in range(1, 9):
        for y in (CORR_S - TI / 2, CORR_N + TI / 2):
            col_item = style(box(-200, -200, 0, 400, 400, h), CONCRETE)
            col = e(f"IFCCOLUMN('{guid()}',$,'Column {tag}.{chr(64 + k)}{1 if y < W / 2 else 2}',$,$,{local(st_pl, k * 2 * M, y, 0)},{body([col_item])},$,.COLUMN.)")
            contained.append(col)
            all_columns.append(col)
            material(col, "Concrete")

    # The roof, with a parapet, on the top floor.
    if li == len(levels) - 1:
        roof_item = style(box(0, 0, height - SLAB, L, W, SLAB), ROOF)
        roof_slab = e(f"IFCSLAB('{guid()}',$,'Roof slab',$,$,{local(st_pl, 0, 0, 0)},{body([roof_item])},$,.ROOF.)")
        roof = e(f"IFCROOF('{guid()}',$,'Roof',$,$,{local(st_pl, 0, 0, 0)},$,$,.FLAT_ROOF.)")
        e(f"IFCRELAGGREGATES('{guid()}',$,$,$,{roof},({roof_slab}))")
        contained.append(roof)
        material(roof_slab, "Concrete")
        pset([roof_slab], "Pset_SlabCommon", {"IsExternal": True})
        pp = local(st_pl, 0, 0, height)
        parapet = [style(box(0, 0, 0, L, T, 900), BRICK), style(box(0, W - T, 0, L, T, 900), BRICK),
                   style(box(0, 0, 0, T, W, 900), BRICK), style(box(L - T, 0, 0, T, W, 900), BRICK)]
        par = e(f"IFCWALL('{guid()}',$,'Parapet',$,$,{pp},{body(parapet)},$,.PARAPET.)")
        material(par, "Brick")
        contained.append(par)
        # A plant room on the roof, above the core.
        plant_pl = local(st_pl, cx0 - M, W - 7200 - T, height)
        plant = [style(box(0, 0, 0, 3 * M + 400, 7200, 3200), CONCRETE), style(box(-200, -200, 3200, 3 * M + 800, 7600, 250), ROOF)]
        pr = e(f"IFCBUILDINGELEMENTPROXY('{guid()}',$,'Plant room',$,$,{plant_pl},{body(plant)},$,.NOTDEFINED.)")
        contained.append(pr)

    e(f"IFCRELCONTAINEDINSPATIALSTRUCTURE('{guid()}',$,$,$,({','.join(contained)}),{storey})")
    if spaces:
        e(f"IFCRELAGGREGATES('{guid()}',$,$,$,{storey},({','.join(spaces)}))")

e(f"IFCRELAGGREGATES('{guid()}',$,$,$,{bldg},({','.join(storeys)}))")
pset(all_ext, "Pset_WallCommon", {"IsExternal": True, "LoadBearing": True, "FireRating": "EI 60", "ThermalTransmittance": 0.18})
pset(all_int, "Pset_WallCommon", {"IsExternal": False, "LoadBearing": False, "FireRating": "EI 30", "AcousticRating": "R'w 44 dB"})
pset(all_windows, "Pset_WindowCommon", {"IsExternal": True, "ThermalTransmittance": 0.9})
pset(all_doors, "Pset_DoorCommon", {"FireRating": "EI2 30-Sa", "HandicapAccessible": True})
pset(all_spaces, "Pset_SpaceCommon", {"IsExternal": False, "PubliclyAccessible": False})
pset(all_columns, "Pset_ColumnCommon", {"LoadBearing": True, "FireRating": "R 90"})
for name, objs in material_of.items():
    e(f"IFCRELASSOCIATESMATERIAL('{guid()}',$,$,$,({','.join(objs)}),{materials[name]})")

out = os.path.join(os.path.dirname(__file__), "..", "examples", "hospital", "hospital.ifc")
os.makedirs(os.path.dirname(out), exist_ok=True)
with open(out, "w", encoding="ascii") as f:
    f.write("ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ViewDefinition [ReferenceView_V1.2]'),'2;1');\n")
    f.write("FILE_NAME('hospital.ifc','2026-11-19T12:00:00',('Ographis'),('Ographis'),'','tools/make_hospital_ifc.py','');\n")
    f.write("FILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n")
    for line in lines:
        # Letters beyond ASCII as STEP writes them.
        f.write("".join(c if ord(c) < 128 else "\\X2\\%04X\\X0\\" % ord(c) for c in line) + "\n")
    f.write("ENDSEC;\nEND-ISO-10303-21;\n")
print(out, len(lines), "records")
