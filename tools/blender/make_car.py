# SPDX-License-Identifier: Apache-2.0
# Builds the drawn model of a car in Blender from its data file: a smooth
# body sized from the same dimensions the simulation uses, with glass,
# lamps, mirrors, dark wheel wells and one wheel per axle, and writes it as
# a glTF binary the Godot front end loads. Drawing only: nothing here feeds
# the simulation. The shapes are original and generic (fictional cars only).
#
#   blender -b --factory-startup -P tools/blender/make_car.py -- \
#       --car data/cars/hatch_tern_gt.toml \
#       --out godot/assets/cars/hatch_tern_gt.glb \
#       [--blend art/cars/hatch_tern_gt.blend] [--render preview.jpg]
#
# Blender axes: X right, Y forward, Z up. The glTF exporter turns Y-forward
# into -Z, which is Godot's forward. The origin is the car's sprung centre of
# gravity, as in the front end's car view.

import argparse
import math
import os
import sys
import tomllib

import bmesh
import bpy
from mathutils import Vector

# Gap between the tire and the wheel arch, m (as in car_body.gd).
ARCH_GAP = 0.06
SUBDIVISIONS = 2


# --- data --------------------------------------------------------------

def value(entry):
    return entry["value"] if isinstance(entry, dict) else entry


def read_toml(path):
    """Reads a data file, following `inherits` as the simulation does."""
    with open(path, "rb") as f:
        table = tomllib.load(f)
    base = table.pop("inherits", None)
    if base is None:
        return table
    merged = read_toml(os.path.join(os.path.dirname(path), base + ".toml"))

    def merge(dst, src):
        for k, v in src.items():
            if isinstance(v, dict) and "value" not in v and isinstance(dst.get(k), dict):
                merge(dst[k], v)
            else:
                dst[k] = v

    merge(merged, table)
    return merged


def car_dimensions(path):
    car = read_toml(path)
    b = car["body"]
    tires_dir = os.path.join(os.path.dirname(path), "..", "tires")
    tires = []
    for key in ("tire_front", "tire_rear"):
        t = read_toml(os.path.join(tires_dir, car[key] + ".toml"))
        tires.append((value(t["geometry"]["unloaded_radius_m"]), value(t["geometry"]["width_m"])))
    return {
        "id": car["id"],
        "electric": car["drivetrain"]["kind"] != "engine_manual",
        "length": value(b["length_m"]),
        "width": value(b["width_m"]),
        "height": value(b["height_m"]),
        "clearance": value(b["ground_clearance_m"]),
        "cg_height": value(b["cg_height_m"]),
        "wheelbase": value(b["wheelbase_m"]),
        "cg_to_front": value(b["cg_to_front_axle_m"]),
        "tracks": (value(car["suspension"]["front"]["track_m"]), value(car["suspension"]["rear"]["track_m"])),
        "tires": tires,
    }


# --- materials ---------------------------------------------------------

def material(name, color, roughness=0.5, metallic=0.0, emission=None, strength=0.0, culled=True):
    m = bpy.data.materials.new(name)
    p = m.node_tree.nodes.get("Principled BSDF")
    p.inputs["Base Color"].default_value = (*color, 1.0)
    p.inputs["Roughness"].default_value = roughness
    p.inputs["Metallic"].default_value = metallic
    if emission is not None:
        p.inputs["Emission Color"].default_value = (*emission, 1.0)
        p.inputs["Emission Strength"].default_value = strength
    # Single-sided, so the driver's-seat camera sees out through the shell.
    m.use_backface_culling = culled
    return m


def make_materials(electric):
    paint = (0.06, 0.22, 0.62) if electric else (0.62, 0.05, 0.035)
    return {
        "paint": material("paint", paint, 0.22, 0.55),
        "glass": material("glass", (0.02, 0.025, 0.03), 0.05, 0.3),
        "trim": material("trim", (0.025, 0.025, 0.028), 0.65),
        "under": material("underbody", (0.015, 0.015, 0.016), 0.9),
        "head": material("headlamp", (0.85, 0.87, 0.9), 0.1, 0.2, (1.0, 0.97, 0.9), 1.5),
        "tail": material("taillamp", (0.45, 0.02, 0.02), 0.15, 0.0, (1.0, 0.05, 0.03), 1.2),
        "plate": material("plate", (0.85, 0.85, 0.82), 0.5),
        "rubber": material("tire", (0.03, 0.03, 0.032), 0.92),
        "alloy": material("alloy", (0.62, 0.63, 0.66), 0.28, 1.0),
        "disc": material("brake_disc", (0.32, 0.31, 0.30), 0.45, 1.0),
    }


# --- body --------------------------------------------------------------

MATS = ["paint", "glass", "trim", "under", "head", "tail", "plate", "rubber", "alloy", "disc"]


def body_design(d):
    """Cross-section stations from nose to tail. Each is
    (distance from the nose m, floor lift, half-width, belt, roof): lift,
    belt and roof are fractions of the body height above the floor, the
    half-width a fraction of half the overall width. Returns the stations
    and the bands that carry glass."""
    L, wb = d["length"], d["wheelbase"]
    over = 0.5 * (L - wb)               # equal overhangs, as the sim's body box
    fa, ra = over, over + wb             # axle stations
    eye = fa + 0.55 * wb                 # driver's eye, as car_view.gd
    screen_base = eye - 0.95
    screen_top = eye - 0.28
    if L < 4.3:
        # Two-box hatch: short bonnet, long roof, steep tailgate.
        rear_top = L - 0.44
        rear_base = L - 0.17
        st = [
            (0.00, 0.20, 0.80, 0.40, 0.43),
            (0.06, 0.08, 0.93, 0.45, 0.48),
            (0.20, 0.02, 0.98, 0.50, 0.53),
            (fa, 0.00, 1.00, 0.53, 0.56),
            (screen_base, 0.00, 1.00, 0.58, 0.60),
            (0.5 * (screen_base + screen_top), 0.00, 1.00, 0.60, 0.83),
            (screen_top, 0.00, 1.00, 0.61, 0.99),
            (eye + 0.30, 0.00, 1.00, 0.62, 1.00),
            (ra - 0.25, 0.00, 1.00, 0.64, 0.985),
            (rear_top, 0.00, 0.99, 0.66, 0.965),
            (L - 0.31, 0.01, 0.975, 0.67, 0.86),
            (rear_base, 0.05, 0.955, 0.665, 0.70),
            (L - 0.06, 0.14, 0.92, 0.64, 0.66),
            (L, 0.24, 0.84, 0.60, 0.62),
        ]
        b_pillar = eye + 0.22
        glass_bands = {"screen": (screen_base, screen_top), "rear": (rear_top, rear_base),
                       "side": (screen_base + 0.05, rear_top - 0.06)}
    else:
        # Three-box fastback sedan: longer bonnet, sloping rear screen, short deck.
        rear_top = ra - 0.30
        rear_base = L - 0.62
        st = [
            (0.00, 0.20, 0.80, 0.38, 0.41),
            (0.06, 0.08, 0.93, 0.43, 0.46),
            (0.22, 0.02, 0.98, 0.48, 0.51),
            (fa, 0.00, 1.00, 0.52, 0.55),
            (screen_base, 0.00, 1.00, 0.57, 0.59),
            (0.5 * (screen_base + screen_top), 0.00, 1.00, 0.59, 0.82),
            (screen_top, 0.00, 1.00, 0.60, 0.98),
            (eye + 0.35, 0.00, 1.00, 0.61, 1.00),
            (rear_top, 0.00, 1.00, 0.63, 0.97),
            (0.5 * (rear_top + rear_base), 0.00, 1.00, 0.64, 0.84),
            (rear_base, 0.00, 0.99, 0.65, 0.69),
            (L - 0.30, 0.03, 0.97, 0.65, 0.68),
            (L - 0.07, 0.14, 0.93, 0.63, 0.65),
            (L, 0.24, 0.85, 0.59, 0.61),
        ]
        b_pillar = eye + 0.25
        glass_bands = {"screen": (screen_base, screen_top), "rear": (rear_top, rear_base),
                       "side": (screen_base + 0.05, rear_base - 0.10)}
    # A narrow pair of stations either side of the B-pillar, so the pillar
    # is its own band.
    for s in (b_pillar - 0.04, b_pillar + 0.04):
        st.append(interpolate(st, s))
    st.sort(key=lambda t: t[0])
    glass_bands["b_pillar"] = (b_pillar - 0.04, b_pillar + 0.04)
    return st, glass_bands


def interpolate(stations, s):
    for a, b in zip(stations, stations[1:]):
        if a[0] <= s <= b[0]:
            u = (s - a[0]) / (b[0] - a[0])
            return tuple(x + (y - x) * u for x, y in zip(a, b))
    raise ValueError(f"station {s} is off the body")


def section(d, station):
    """Right half of a cross-section, bottom centre round to roof centre,
    as (x, z) pairs."""
    s, lift, hwf, belt, roof = station
    H = d["height"]
    hw = 0.5 * d["width"] * hwf
    zf = -(d["cg_height"] - d["clearance"])
    zl = zf + lift * H
    zb = zf + belt * H
    zr = zf + roof * H
    g = roof - belt
    k = max(0.0, min(1.0, g / 0.3))

    def lerp(a, b, t):
        return a + (b - a) * t

    return [
        (0.0, zl),
        (0.78 * hw, zl),
        (0.96 * hw, zl + 0.03),
        (0.995 * hw, max(zl + 0.12, zf + 0.12)),
        (1.000 * hw, lerp(max(zl + 0.12, zf + 0.12), zb, 0.55)),
        (0.975 * hw, zb - 0.03),
        (0.93 * hw, zb),
        (lerp(0.92, 0.77, k) * hw, zb + 0.82 * g * H),
        (lerp(0.86, 0.70, k) * hw, zb + 0.94 * g * H),
        (0.0, zr),
    ]


def build_body(d, mats):
    L, wb, a = d["length"], d["wheelbase"], d["cg_to_front"]
    y_nose = (a - 0.5 * wb) + 0.5 * L
    stations, bands = body_design(d)
    rings = []
    for st in stations:
        y = y_nose - st[0]
        rings.append([Vector((x, y, z)) for x, z in section(d, st)])

    # Close each end with rings shrinking toward the middle of the face.
    def cap_rings(ring, dy):
        zc = 0.5 * (ring[0].z + ring[-1].z)
        out = []
        for f, off in ((0.72, 0.012), (0.40, 0.018)):
            out.append([Vector((p.x * f, p.y + dy * off, zc + (p.z - zc) * f)) for p in ring])
        return out

    front_caps = cap_rings(rings[0], 1.0)
    rear_caps = cap_rings(rings[-1], -1.0)
    all_rings = list(reversed(front_caps)) + rings + rear_caps
    n_front = len(front_caps)

    mesh = bpy.data.meshes.new("body")
    bm = bmesh.new()
    verts = [[bm.verts.new(p) for p in ring] for ring in all_rings]
    npts = len(rings[0])

    def band_material(i, j):
        """Material of the band between body stations i and i+1, segment j."""
        if i < 0 or i >= len(stations) - 1:
            return "paint"
        s0, s1 = stations[i][0], stations[i + 1][0]
        mid = 0.5 * (s0 + s1)
        if j == 0:
            return "under"
        if j == 6 and bands["side"][0] <= mid <= bands["side"][1]:
            lo, hi = bands["b_pillar"]
            return "trim" if lo - 1e-6 <= s0 and s1 <= hi + 1e-6 else "glass"
        if j == 8:
            lo, hi = bands["screen"]
            if lo - 1e-6 <= s0 and s1 <= hi + 1e-6:
                return "glass"
            lo, hi = bands["rear"]
            if lo - 1e-6 <= s0 and s1 <= hi + 1e-6:
                return "glass"
        return "paint"

    for r in range(len(all_rings) - 1):
        i = r - n_front
        for j in range(npts - 1):
            f = bm.faces.new((verts[r][j], verts[r][j + 1], verts[r + 1][j + 1], verts[r + 1][j]))
            f.material_index = MATS.index(band_material(i, j))
    # Crease the belt line and the sill so the subdivided body keeps a
    # shoulder and a rocker edge instead of rounding into a blob.
    crease = bm.edges.layers.float.get("crease_edge") or bm.edges.layers.float.new("crease_edge")
    for r in range(n_front, n_front + len(rings) - 1):
        for j, c in ((6, 0.75), (5, 0.35), (3, 0.5)):
            e = bm.edges.get((verts[r][j], verts[r + 1][j]))
            if e is not None:
                e[crease] = c
    for ring, m in ((verts[0], "paint"), (verts[-1], "paint")):
        f = bm.faces.new(ring)
        f.material_index = MATS.index(m)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(mesh)
    bm.free()

    obj = bpy.data.objects.new("body", mesh)
    bpy.context.collection.objects.link(obj)
    for name in MATS:
        mesh.materials.append(mats[name])
    mirror = obj.modifiers.new("mirror", "MIRROR")
    mirror.use_clip = True
    mirror.use_mirror_merge = True
    mirror.merge_threshold = 0.001
    sub = obj.modifiers.new("subsurf", "SUBSURF")
    sub.levels = SUBDIVISIONS
    sub.render_levels = SUBDIVISIONS
    apply_modifiers(obj)
    return obj, y_nose, stations, bands


def apply_modifiers(obj):
    bpy.context.view_layer.objects.active = obj
    for m in list(obj.modifiers):
        bpy.ops.object.modifier_apply(modifier=m.name)


def axles(d):
    a, wb = d["cg_to_front"], d["wheelbase"]
    out = []
    for i, y in enumerate((a, a - wb)):
        r, w = d["tires"][i]
        out.append({"y": y, "z": r - d["cg_height"], "r": r, "w": w, "track": d["tracks"][i]})
    return out


def cut_arches(d, body):
    """Cuts a wheel arch over each tire. The cut leaves faces closing the
    shell round the wheel (the arch's roof and inner wall); they become
    underbody, so the arch reads as a dark well."""
    hw = 0.5 * d["width"]
    wells = []
    for ax in axles(d):
        R = ax["r"] + ARCH_GAP
        inner = max(0.5 * ax["track"] - 0.5 * ax["w"] - 0.08, 0.15)
        wells.append((ax, R, inner))
        for side in (1.0, -1.0):
            bpy.ops.mesh.primitive_cylinder_add(vertices=48, radius=R, depth=hw + 0.3 - inner,
                                                location=(side * 0.5 * (inner + hw + 0.3), ax["y"], ax["z"]),
                                                rotation=(0.0, math.pi / 2, 0.0))
            cutter = bpy.context.active_object
            boolean = body.modifiers.new("arch", "BOOLEAN")
            boolean.operation = "DIFFERENCE"
            boolean.solver = "EXACT"
            boolean.object = cutter
            apply_modifiers(body)
            bpy.data.objects.remove(cutter, do_unlink=True)
    under = MATS.index("under")
    for poly in body.data.polygons:
        c = poly.center
        for ax, R, inner in wells:
            if abs(c.x) < inner - 0.003 or math.hypot(c.y - ax["y"], c.z - ax["z"]) > R + 0.004:
                continue
            n = poly.normal
            toward_axle = Vector((0.0, ax["y"] - c.y, ax["z"] - c.z)).normalized()
            inner_wall = abs(abs(c.x) - inner) < 0.01 and abs(n.x) > 0.9
            if inner_wall or n.dot(toward_axle) > 0.5:
                poly.material_index = under


# --- details projected onto the body ------------------------------------

def project_patch(body, name, mat, corners, direction, nu=8, nv=4, lift=0.004):
    """A thin panel conforming to the body: a grid between four corner
    points (on a plane in front of the body), each vertex cast along
    `direction` onto the body and lifted off it along the surface normal."""
    depsgraph = bpy.context.evaluated_depsgraph_get()
    inv = body.matrix_world.inverted()
    c00, c10, c11, c01 = [Vector(c) for c in corners]
    grid = []
    for v in range(nv + 1):
        row = []
        for u in range(nu + 1):
            p = (c00 * (1 - u / nu) + c10 * (u / nu)) * (1 - v / nv) + (c01 * (1 - u / nu) + c11 * (u / nu)) * (v / nv)
            hit, loc, normal, _ = body.ray_cast(inv @ p, Vector(direction), depsgraph=depsgraph)
            if not hit:
                return None
            row.append(loc + normal * lift)
        grid.append(row)
    mesh = bpy.data.meshes.new(name)
    bm = bmesh.new()
    vs = [[bm.verts.new(p) for p in row] for row in grid]
    for v in range(nv):
        for u in range(nu):
            bm.faces.new((vs[v][u], vs[v][u + 1], vs[v + 1][u + 1], vs[v + 1][u]))
    bm.normal_update()
    outward = -Vector(direction)
    for f in bm.faces:
        if f.normal.dot(outward) < 0:
            f.normal_flip()
    bm.to_mesh(mesh)
    bm.free()
    mesh.materials.append(mat)
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    for poly in mesh.polygons:
        poly.use_smooth = True
    return obj


def add_details(d, body, mats, y_nose, stations, bands):
    H = d["height"]
    hw = 0.5 * d["width"]
    zf = -(d["cg_height"] - d["clearance"])
    y_tail = y_nose - d["length"]
    front_belt = zf + stations[1][3] * H
    rear_belt = zf + stations[-2][3] * H
    parts = []
    ahead = y_nose + 0.5
    behind = y_tail - 0.5
    for side in (1.0, -1.0):
        # Headlamp: a band across the upper corner of the nose.
        x0, x1 = side * 0.42 * hw, side * 0.86 * hw
        parts.append(project_patch(body, "headlamp", mats["head"],
                                   [(x0, ahead, front_belt - 0.10), (x1, ahead, front_belt - 0.07),
                                    (x1, ahead, front_belt - 0.01), (x0, ahead, front_belt - 0.03)], (0, -1, 0)))
        # Tail lamp, across the rear corner.
        x0, x1 = side * 0.50 * hw, side * 0.90 * hw
        parts.append(project_patch(body, "taillamp", mats["tail"],
                                   [(x0, behind, rear_belt - 0.13), (x1, behind, rear_belt - 0.12),
                                    (x1, behind, rear_belt - 0.04), (x0, behind, rear_belt - 0.05)], (0, 1, 0)))
    # Lower intake, front plate and rear plate.
    zi = zf + stations[1][1] * H
    parts.append(project_patch(body, "intake", mats["trim"],
                               [(-0.62 * hw, ahead, zi + 0.05), (0.62 * hw, ahead, zi + 0.05),
                                (0.62 * hw, ahead, zi + 0.19), (-0.62 * hw, ahead, zi + 0.19)], (0, -1, 0), nu=12))
    parts.append(project_patch(body, "grille", mats["trim"],
                               [(-0.34 * hw, ahead, front_belt - 0.16), (0.34 * hw, ahead, front_belt - 0.16),
                                (0.34 * hw, ahead, front_belt - 0.06), (-0.34 * hw, ahead, front_belt - 0.06)], (0, -1, 0), nu=10))
    parts.append(project_patch(body, "plate", mats["plate"],
                               [(-0.24, behind, rear_belt - 0.36), (0.24, behind, rear_belt - 0.36),
                                (0.24, behind, rear_belt - 0.24), (-0.24, behind, rear_belt - 0.24)], (0, 1, 0), lift=0.006))
    parts.append(project_patch(body, "rear_valance", mats["trim"],
                               [(-0.80 * hw, behind, zf + stations[-2][1] * H + 0.03), (0.80 * hw, behind, zf + stations[-2][1] * H + 0.03),
                                (0.80 * hw, behind, zf + stations[-2][1] * H + 0.13), (-0.80 * hw, behind, zf + stations[-2][1] * H + 0.13)], (0, 1, 0), nu=12))
    # Black sills along the bottom of each side, between the arches.
    ax = axles(d)
    y0 = ax[0]["y"] - ax[0]["r"] - ARCH_GAP - 0.02
    y1 = ax[1]["y"] + ax[1]["r"] + ARCH_GAP + 0.02
    for side in (1.0, -1.0):
        out = side * (hw + 0.5)
        parts.append(project_patch(body, "sill", mats["trim"],
                                   [(out, y0, zf + 0.02), (out, y1, zf + 0.02), (out, y1, zf + 0.10), (out, y0, zf + 0.10)],
                                   (-side, 0, 0), nu=12, nv=2))
    # Door mirrors at the base of the windscreen.
    ys = y_nose - bands["screen"][0] - 0.12
    zb = zf + stations[4][3] * H
    for side in (1.0, -1.0):
        bpy.ops.mesh.primitive_cube_add(size=1.0, location=(side * (hw * 0.88 + 0.085), ys, zb + 0.09))
        m = bpy.context.active_object
        m.name = "mirror"
        m.scale = (0.17, 0.08, 0.10)
        bpy.ops.object.transform_apply(scale=True)
        bev = m.modifiers.new("bevel", "BEVEL")
        bev.width = 0.025
        bev.segments = 3
        apply_modifiers(m)
        m.data.materials.append(mats["paint"])
        bpy.ops.object.shade_smooth()
        parts.append(m)
    return [p for p in parts if p is not None]


# --- wheels ------------------------------------------------------------

def lathe(name, profile, mat, segments=48):
    """Spins a (radius, x) profile about the X axis."""
    mesh = bpy.data.meshes.new(name)
    bm = bmesh.new()
    rings = []
    for k in range(segments):
        t = 2 * math.pi * k / segments
        rings.append([bm.verts.new((x, r * math.cos(t), r * math.sin(t))) for r, x in profile])
    for k in range(segments):
        a, b = rings[k], rings[(k + 1) % segments]
        for j in range(len(profile) - 1):
            bm.faces.new((a[j], a[j + 1], b[j + 1], b[j]))
    bm.normal_update()
    bm.to_mesh(mesh)
    bm.free()
    mesh.materials.append(mat)
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    for poly in mesh.polygons:
        poly.use_smooth = True
    return obj


def build_wheel(name, radius, width, mats):
    """One wheel, axle along X, outer face toward +X, centred on the origin."""
    rim_r = radius - 0.40 * width * 0.98 if radius - 0.40 * width > 0.15 else radius * 0.7
    hw = 0.5 * width
    # Tire: inner bead, sidewall, rounded shoulder, tread and back (outside first).
    tire = [
        (rim_r - 0.005, -hw * 0.88), (rim_r + 0.25 * (radius - rim_r), -hw * 1.0),
        (rim_r + 0.75 * (radius - rim_r), -hw * 0.98), (radius - 0.012, -hw * 0.86),
        (radius, -hw * 0.6), (radius, hw * 0.6), (radius - 0.012, hw * 0.86),
        (rim_r + 0.75 * (radius - rim_r), hw * 0.98), (rim_r + 0.25 * (radius - rim_r), hw * 1.0),
        (rim_r - 0.005, hw * 0.88),
    ]
    parts = [lathe("tire", list(reversed(tire)), mats["rubber"], 64)]
    # Rim barrel and lip.
    lip = [(rim_r - 0.012, -hw * 0.86), (rim_r, -hw * 0.9), (rim_r + 0.008, -hw * 0.9),
           (rim_r + 0.008, hw * 0.9), (rim_r, hw * 0.92), (rim_r - 0.022, hw * 0.84)]
    parts.append(lathe("rim", lip, mats["alloy"], 64))
    # Hub face, set back from the lip.
    hub = [(0.0, hw * 0.62), (0.07 * radius, hw * 0.64), (0.17 * radius, hw * 0.58)]
    parts.append(lathe("hub", hub, mats["alloy"], 32))
    # Brake disc behind the spokes.
    disc_r = rim_r - 0.035
    disc = [(0.35 * disc_r, hw * 0.05), (disc_r, hw * 0.05), (disc_r, -hw * 0.2), (0.35 * disc_r, -hw * 0.2)]
    parts.append(lathe("disc", disc, mats["disc"], 48))
    # Ten spokes in five pairs, from the hub to the lip, dished outward.
    for k in range(10):
        t = 2 * math.pi * (k // 2) / 5 + (0.17 if k % 2 else -0.17)
        bpy.ops.mesh.primitive_cube_add(size=1.0)
        s = bpy.context.active_object
        s.name = "spoke"
        length = rim_r - 0.15 * radius
        s.scale = (0.045, 0.034, length)
        bpy.ops.object.transform_apply(scale=True)
        for v in s.data.vertices:
            # Taper toward the lip and dish: inner end proud, outer end back.
            u = (v.co.z + 0.5 * length) / length
            v.co.y *= 1.0 - 0.35 * u
            v.co.x += hw * (0.60 - 0.28 * u)
            v.co.z += 0.15 * radius + 0.5 * length
        s.rotation_euler = (t, 0.0, 0.0)
        bpy.ops.object.transform_apply(rotation=True)
        s.data.materials.append(mats["alloy"])
        parts.append(s)
    bpy.ops.object.select_all(action="DESELECT")
    for p in parts:
        p.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.join()
    wheel = bpy.context.active_object
    wheel.name = name
    wheel.data.name = name
    return wheel


# --- scene -------------------------------------------------------------

def preview(path, d, wheels):
    """Renders the car from three angles: `path` with -front, -side and
    -rear before the extension."""
    scene = bpy.context.scene
    placed = []
    for ax_i, ax in enumerate(axles(d)):
        for side in (1.0, -1.0):
            w = wheels[ax_i].copy()
            w.location = (side * 0.5 * ax["track"], ax["y"], ax["z"])
            w.scale = (side, 1.0, 1.0)
            bpy.context.collection.objects.link(w)
            placed.append(w)
    for w in wheels:
        w.hide_render = True
    ground_z = -d["cg_height"]
    bpy.ops.mesh.primitive_plane_add(size=60, location=(0, 0, ground_z))
    ground = bpy.context.active_object
    ground.data.materials.append(material("ground", (0.18, 0.18, 0.19), 0.8, culled=False))
    cam_data = bpy.data.cameras.new("camera")
    cam_data.lens = 50
    cam = bpy.data.objects.new("camera", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    sun_data = bpy.data.lights.new("sun", "SUN")
    sun_data.energy = 3.5
    sun_data.angle = 0.1
    sun = bpy.data.objects.new("sun", sun_data)
    sun.rotation_euler = (math.radians(50), math.radians(10), math.radians(140))
    scene.collection.objects.link(sun)
    world = bpy.data.worlds.new("world")
    world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.55, 0.62, 0.72, 1.0)
    world.node_tree.nodes["Background"].inputs["Strength"].default_value = 0.8
    scene.world = world
    scene.render.resolution_x = 1280
    scene.render.resolution_y = 720
    if path.lower().endswith((".jpg", ".jpeg")):
        scene.render.image_settings.file_format = "JPEG"
        scene.render.image_settings.quality = 85
    for engine in ("BLENDER_EEVEE", "BLENDER_EEVEE_NEXT"):
        try:
            scene.render.engine = engine
            break
        except TypeError:
            continue
    target = Vector((0.0, 0.0, ground_z + 0.65))
    stem, ext = os.path.splitext(path)
    for view, offset in (("front", (4.6, 5.8, 1.9)), ("side", (8.5, 0.0, 0.9)), ("rear", (-4.4, -5.9, 2.2))):
        cam.location = target + Vector(offset)
        cam.rotation_euler = (target - cam.location).to_track_quat("-Z", "Y").to_euler()
        scene.render.filepath = f"{stem}-{view}{ext}"
        bpy.ops.render.render(write_still=True)
    for o in placed + [ground, cam, sun]:
        bpy.data.objects.remove(o, do_unlink=True)
    for w in wheels:
        w.hide_render = False


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    ap = argparse.ArgumentParser()
    ap.add_argument("--car", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--blend")
    ap.add_argument("--render")
    args = ap.parse_args(argv)

    d = car_dimensions(args.car)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    mats = make_materials(d["electric"])
    body, y_nose, stations, bands = build_body(d, mats)
    cut_arches(d, body)
    bpy.context.view_layer.objects.active = body
    body.select_set(True)
    bpy.ops.object.shade_smooth_by_angle(angle=math.radians(35))
    details = add_details(d, body, mats, y_nose, stations, bands)
    wheels = [build_wheel("wheel_front", *d["tires"][0], mats),
              build_wheel("wheel_rear", *d["tires"][1], mats)]
    for o in [body] + details:
        o.select_set(False)

    if args.render:
        preview(os.path.abspath(args.render), d, wheels)

    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    bpy.ops.export_scene.gltf(filepath=os.path.abspath(args.out), export_format="GLB",
                              export_yup=True, export_apply=True, export_cameras=False,
                              export_lights=False)
    if args.blend:
        os.makedirs(os.path.dirname(os.path.abspath(args.blend)), exist_ok=True)
        bpy.ops.wm.save_as_mainfile(filepath=os.path.abspath(args.blend))
    tris = sum(len(o.data.polygons) for o in bpy.data.objects if o.type == "MESH")
    print(f"QTS: wrote {args.out} ({tris} faces)")


main()
