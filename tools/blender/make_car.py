# SPDX-License-Identifier: Apache-2.0
# Builds the drawn model of a car in Blender from its data file: a smooth
# body sized from the same dimensions the simulation uses, with glass,
# lamps, grilles, mirrors, panel lines, handles, wheel wells, and one wheel
# and brake caliper per axle, and writes it as a glTF binary the Godot front
# end loads. Drawing only: nothing here feeds the simulation.
#
# Each car is drawn in a style that says what kind of car it stands for
# (D-092, D-094): the Tern GT as a small upright three-door hatch with
# round lamps and a contrasting roof, the Halden E4 as an electric sport
# sedan. The shapes and details are original: no maker's badge, grille,
# lamp graphic or name is copied.
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

# Styles by car id. A car not listed gets "hatch" under 4.3 m, else "sedan".
# "small" is the upright three-door (the Tern GT since D-094); "hatch" the
# five-door hot hatch it was drawn as before.
STYLES = {
    "hatch_tern_gt": "small",
    "sedan_halden_e4": "sedan",
}


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
    length = value(b["length_m"])
    wheelbase = value(b["wheelbase_m"])
    # Front axle to nose: the data's figure, else equal overhangs, as the
    # simulation's body box (BodyParams::front_overhang).
    front_overhang = value(b["front_overhang_m"]) if "front_overhang_m" in b else 0.5 * (length - wheelbase)
    return {
        "id": car["id"],
        "style": STYLES.get(car["id"], "hatch" if length < 4.3 else "sedan"),
        "length": length,
        "width": value(b["width_m"]),
        "height": value(b["height_m"]),
        "clearance": value(b["ground_clearance_m"]),
        "cg_height": value(b["cg_height_m"]),
        "wheelbase": wheelbase,
        "front_overhang": front_overhang,
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


def honeycomb_material():
    """A mesh grille: dark hexagonal openings in a black lattice, drawn
    into a small tiling image packed into the model."""
    w, h = 64, 111                       # one tile: two cells across
    centres = []
    for i in range(-1, 3):
        for j in range(-1, 3):
            centres.append((i * w * 0.5, j * h))
            centres.append((i * w * 0.5 + w * 0.25, j * h + h * 0.5))
    pixels = []
    for y in range(h):
        for x in range(w):
            ds = sorted(math.hypot(x - cx, y - cy) for cx, cy in centres)
            edge = 0.5 * (ds[1] - ds[0])
            v = 0.006 if edge > 2.2 else 0.05
            pixels.extend((v, v, v * 1.05, 1.0))
    img = bpy.data.images.new("honeycomb", w, h)
    img.pixels = pixels
    img.pack()
    m = material("grille_mesh", (1.0, 1.0, 1.0), 0.55)
    nodes = m.node_tree.nodes
    tex = nodes.new("ShaderNodeTexImage")
    tex.image = img
    tex.interpolation = "Linear"
    m.node_tree.links.new(tex.outputs["Color"], nodes["Principled BSDF"].inputs["Base Color"])
    return m


def make_materials(style):
    if style == "small":
        paint, alloy, caliper = (0.55, 0.02, 0.018), (0.62, 0.63, 0.66), (0.16, 0.16, 0.17)
    elif style == "hatch":
        paint, alloy, caliper = (0.58, 0.03, 0.025), (0.60, 0.61, 0.64), (0.62, 0.03, 0.02)
    else:
        paint, alloy, caliper = (0.05, 0.20, 0.58), (0.13, 0.135, 0.145), (0.62, 0.03, 0.02)
    return {
        "paint": material("paint", paint, 0.2, 0.55),
        "roof": material("roof", (0.86, 0.86, 0.84), 0.22, 0.1),
        "amber": material("indicator", (0.8, 0.4, 0.02), 0.15, 0.0, (1.0, 0.5, 0.05), 0.4),
        "glass": material("glass", (0.015, 0.02, 0.025), 0.04, 0.4),
        "trim": material("trim", (0.02, 0.02, 0.022), 0.6),
        "gloss_black": material("gloss_black", (0.01, 0.01, 0.012), 0.12, 0.2),
        "under": material("underbody", (0.012, 0.012, 0.013), 0.9),
        "seam": material("panel_gap", (0.008, 0.008, 0.008), 0.9),
        "housing": material("lamp_housing", (0.05, 0.055, 0.06), 0.08, 0.8),
        "head": material("headlamp", (0.9, 0.92, 0.95), 0.1, 0.2, (1.0, 0.97, 0.9), 2.0),
        "lens": material("lamp_lens", (0.45, 0.46, 0.48), 0.06, 1.0),
        "tail": material("taillamp", (0.35, 0.01, 0.01), 0.12, 0.0, (0.9, 0.03, 0.02), 0.8),
        "tail_bright": material("taillamp_bar", (0.8, 0.05, 0.04), 0.1, 0.0, (1.0, 0.08, 0.04), 2.5),
        "accent": material("accent", (0.7, 0.02, 0.02), 0.3),
        "chrome": material("chrome", (0.8, 0.8, 0.82), 0.08, 1.0),
        "plate": material("plate", (0.85, 0.85, 0.82), 0.5),
        "honeycomb": honeycomb_material(),
        "rubber": material("tire", (0.03, 0.03, 0.032), 0.92),
        "alloy": material("alloy", alloy, 0.28, 1.0),
        "disc": material("brake_disc", (0.32, 0.31, 0.30), 0.45, 1.0),
        "caliper": material("caliper", caliper, 0.35, 0.1),
    }


# --- body --------------------------------------------------------------

BODY_MATS = ["paint", "glass", "under", "gloss_black", "roof"]


def body_design(d):
    """Cross-section stations from nose to tail. Each is
    (distance from the nose m, floor lift, half-width, belt, roof): lift,
    belt and roof are fractions of the body height above the floor, the
    half-width a fraction of half the overall width. Also returns the
    places the details are drawn at, as distances from the nose."""
    L, wb = d["length"], d["wheelbase"]
    fa, ra = d["front_overhang"], d["front_overhang"] + wb   # axle stations, as the sim's body box
    eye = fa + 0.55 * wb                 # driver's eye, as car_view.gd
    if d["style"] == "small":
        # Small upright three-door: short bonnet with an upright face, an
        # upright screen, tall flat sides with a level belt, little
        # tumblehome, a flat roof running nearly to the tail over a
        # near-vertical tailgate. One long door each side.
        sb, st_ = eye - 0.95, eye - 0.42
        rear_top, rear_base = L - 0.30, L - 0.07
        st = [
            (0.00, 0.18, 0.70, 0.45, 0.47),
            (0.05, 0.07, 0.85, 0.51, 0.535),
            (0.16, 0.015, 0.945, 0.555, 0.58),
            (0.34, 0.00, 0.985, 0.58, 0.605),
            (fa, 0.00, 1.00, 0.595, 0.62),
            (sb, 0.00, 1.00, 0.62, 0.645),
            (0.5 * (sb + st_), 0.00, 1.00, 0.63, 0.84),
            (st_, 0.00, 1.00, 0.635, 0.985),
            (eye + 0.30, 0.00, 1.00, 0.64, 1.00),
            (ra - 0.15, 0.00, 1.00, 0.645, 0.995),
            (rear_top, 0.00, 0.985, 0.65, 0.975),
            (L - 0.16, 0.03, 0.955, 0.655, 0.83),
            (rear_base, 0.08, 0.91, 0.65, 0.68),
            (L - 0.02, 0.15, 0.85, 0.63, 0.65),
            (L, 0.24, 0.74, 0.59, 0.61),
        ]
        b = eye + 0.32
        q = None
        windows = [
            [(sb + 0.12, 0.04), (b - 0.04, 0.04), (b - 0.04, 0.93), (st_ + 0.05, 0.93)],
            [(b + 0.04, 0.04), (rear_top - 0.05, 0.04), (rear_top - 0.26, 0.93), (b + 0.04, 0.93)],
        ]
    elif d["style"] == "hatch":
        # Hot hatch: upright nose, near-horizontal bonnet, long flat roof
        # ending in a spoiler over a steep tailgate, thick C-pillar.
        sb, st_ = eye - 0.95, eye - 0.28
        rear_top, rear_base = L - 0.44, L - 0.17
        st = [
            (0.00, 0.17, 0.82, 0.43, 0.46),
            (0.05, 0.06, 0.94, 0.48, 0.51),
            (0.18, 0.01, 0.985, 0.53, 0.56),
            (fa, 0.00, 1.00, 0.565, 0.59),
            (sb, 0.00, 1.00, 0.60, 0.62),
            (0.5 * (sb + st_), 0.00, 1.00, 0.615, 0.84),
            (st_, 0.00, 1.00, 0.625, 0.99),
            (eye + 0.30, 0.00, 1.00, 0.635, 1.00),
            (ra - 0.25, 0.00, 1.00, 0.65, 0.99),
            (rear_top, 0.00, 0.99, 0.665, 0.975),
            (L - 0.31, 0.01, 0.975, 0.675, 0.87),
            (rear_base, 0.05, 0.955, 0.67, 0.70),
            (L - 0.06, 0.14, 0.92, 0.645, 0.665),
            (L, 0.24, 0.84, 0.60, 0.62),
        ]
        b = eye + 0.22
        q = ra - 0.30                    # rear door's rear edge, at the glass
        windows = [
            [(sb + 0.13, 0.04), (b - 0.04, 0.04), (b - 0.04, 0.92), (st_ + 0.06, 0.92)],
            [(b + 0.04, 0.04), (q - 0.02, 0.04), (q - 0.02, 0.92), (b + 0.04, 0.92)],
            # Small quarter light; the thick pillar behind it leans forward.
            [(q + 0.04, 0.04), (q + 0.26, 0.04), (q + 0.12, 0.92), (q + 0.04, 0.92)],
        ]
    else:
        # Electric sport sedan: low rounded nose with no upper grille, very
        # raked screen, one sweep of glass over the roof, fastback into a
        # short tail with a small lip.
        sb, st_ = eye - 1.05, eye - 0.25
        rear_top, rear_base = ra - 0.45, L - 0.55
        st = [
            (0.00, 0.20, 0.80, 0.34, 0.37),
            (0.07, 0.07, 0.93, 0.40, 0.43),
            (0.25, 0.02, 0.98, 0.46, 0.49),
            (fa, 0.00, 1.00, 0.50, 0.53),
            (sb, 0.00, 1.00, 0.555, 0.575),
            (sb + 0.33 * (st_ - sb), 0.00, 1.00, 0.575, 0.75),
            (sb + 0.67 * (st_ - sb), 0.00, 1.00, 0.59, 0.90),
            (st_, 0.00, 1.00, 0.60, 0.98),
            (eye + 0.35, 0.00, 1.00, 0.61, 1.00),
            (rear_top, 0.00, 1.00, 0.625, 0.965),
            (0.5 * (rear_top + rear_base), 0.00, 1.00, 0.635, 0.84),
            (rear_base, 0.00, 0.99, 0.645, 0.675),
            (L - 0.30, 0.03, 0.975, 0.655, 0.685),
            (L - 0.07, 0.14, 0.93, 0.635, 0.655),
            (L, 0.24, 0.85, 0.59, 0.61),
        ]
        b = eye + 0.25
        q = ra - 0.25
        windows = [
            [(sb + 0.16, 0.04), (b - 0.04, 0.04), (b - 0.04, 0.92), (st_ + 0.06, 0.92)],
            [(b + 0.04, 0.04), (q - 0.02, 0.04), (q - 0.16, 0.86), (b + 0.04, 0.92)],
            [(q + 0.04, 0.04), (q + 0.42, 0.04), (q + 0.04, 0.80), (q + 0.04, 0.80)],
        ]
    places = {"fa": fa, "ra": ra, "eye": eye, "screen": (sb, st_), "rear": (rear_top, rear_base),
              "b_pillar": b, "rear_door": q, "windows": windows,
              "roof_glass": d["style"] == "sedan",
              # Black pillars under a roof in its own colour.
              "floating_roof": d["style"] == "small"}
    # Side glass leaning in toward the roof: the share of the half-width at
    # the glass top and at the roof edge.
    d["tumble"] = (0.82, 0.72) if d["style"] == "small" else (0.77, 0.70)
    return st, places


def interpolate(stations, s):
    s = max(stations[0][0], min(stations[-1][0], s))
    for a, b in zip(stations, stations[1:]):
        if a[0] <= s <= b[0]:
            u = (s - a[0]) / max(b[0] - a[0], 1e-9)
            return tuple(x + (y - x) * u for x, y in zip(a, b))
    return stations[-1]


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

    tg, tr = d.get("tumble", (0.77, 0.70))
    if d["style"] == "small":
        low = max(zl + 0.16, zf + 0.16)
        return [
            (0.0, zl),
            (0.72 * hw, zl),
            (0.93 * hw, zl + 0.045),
            (0.985 * hw, low),
            (1.000 * hw, lerp(low, zb, 0.6)),
            (0.965 * hw, zb - 0.05),
            (0.915 * hw, zb),
            (lerp(0.90, tg, k) * hw, zb + 0.80 * g * H),
            (lerp(0.84, tr, k) * hw, zb + 0.93 * g * H),
            (0.0, zr),
        ]
    return [
        (0.0, zl),
        (0.78 * hw, zl),
        (0.96 * hw, zl + 0.03),
        (0.995 * hw, max(zl + 0.12, zf + 0.12)),
        (1.000 * hw, lerp(max(zl + 0.12, zf + 0.12), zb, 0.55)),
        (0.975 * hw, zb - 0.03),
        (0.93 * hw, zb),
        (lerp(0.92, d.get("tumble", (0.77, 0.70))[0], k) * hw, zb + 0.82 * g * H),
        (lerp(0.86, d.get("tumble", (0.77, 0.70))[1], k) * hw, zb + 0.94 * g * H),
        (0.0, zr),
    ]


class Frame:
    """Converts between distance from the nose and the model's axes, and
    gives heights along the body."""

    def __init__(self, d, stations):
        self.d = d
        self.stations = stations
        self.H = d["height"]
        self.hw = 0.5 * d["width"]
        self.zf = -(d["cg_height"] - d["clearance"])
        self.y_nose = d["cg_to_front"] + d["front_overhang"]

    def y(self, s):
        return self.y_nose - s

    def z(self, frac):
        return self.zf + frac * self.H

    def belt(self, s):
        return self.z(interpolate(self.stations, s)[3])

    def roof(self, s):
        return self.z(interpolate(self.stations, s)[4])

    def lift(self, s):
        return self.z(interpolate(self.stations, s)[1])

    def glass_top(self, s):
        st = interpolate(self.stations, s)
        return self.z(st[3]) + 0.82 * (st[4] - st[3]) * self.H

    def window_point(self, s, f, side):
        """A point on the side glass: `f` 0 at the belt, 1 at the glass top."""
        zb = self.belt(s)
        return Vector((side * self.hw, self.y(s), zb + f * (self.glass_top(s) - zb)))


def build_body(d, mats, frame, stations, places):
    rings = []
    for st in stations:
        y = frame.y(st[0])
        rings.append([Vector((x, y, z)) for x, z in section(d, st)])

    # Close each end with rings shrinking toward the middle of the face.
    def cap_rings(ring, dy):
        zc = 0.5 * (ring[0].z + ring[-1].z)
        return [[Vector((p.x * f, p.y + dy * off, zc + (p.z - zc) * f)) for p in ring]
                for f, off in ((0.72, 0.012), (0.40, 0.018))]

    front_caps = cap_rings(rings[0], 1.0)
    rear_caps = cap_rings(rings[-1], -1.0)
    all_rings = list(reversed(front_caps)) + rings + rear_caps
    n_front = len(front_caps)

    mesh = bpy.data.meshes.new("body")
    bm = bmesh.new()
    verts = [[bm.verts.new(p) for p in ring] for ring in all_rings]
    npts = len(rings[0])

    def band_material(i, j):
        """Material of the band between body stations i and i+1, segment j.
        Side windows are drawn on top as panels; the screens and, on the
        sedan, the roof are glass bands."""
        if i < 0 or i >= len(stations) - 1:
            return "paint"
        s0, s1 = stations[i][0], stations[i + 1][0]
        if j == 0:
            return "under"
        if places["floating_roof"] and j in (6, 7):
            lo, hi = places["screen"][0], places["rear"][0]
            if lo - 1e-6 <= s0 and s1 <= hi + 1e-6:
                return "gloss_black"
        if j == 8:
            lo, hi = places["screen"]
            rlo, rhi = places["rear"]
            if places["roof_glass"]:
                hi = rlo
            for lo_, hi_ in ((lo, hi), (rlo, rhi)):
                if lo_ - 1e-6 <= s0 and s1 <= hi_ + 1e-6:
                    return "glass"
            if places["floating_roof"] and hi - 1e-6 <= s0 and s1 <= rlo + 1e-6:
                return "roof"
        return "paint"

    for r in range(len(all_rings) - 1):
        i = r - n_front
        for j in range(npts - 1):
            f = bm.faces.new((verts[r][j], verts[r][j + 1], verts[r + 1][j + 1], verts[r + 1][j]))
            f.material_index = BODY_MATS.index(band_material(i, j))
    # Crease the belt line and the sill so the subdivided body keeps a
    # shoulder and a rocker edge instead of rounding into a blob.
    crease = bm.edges.layers.float.get("crease_edge") or bm.edges.layers.float.new("crease_edge")
    creases = ((6, 0.35), (5, 0.1), (3, 0.15)) if d["style"] == "small" else ((6, 0.75), (5, 0.45), (3, 0.5))
    for r in range(n_front, n_front + len(rings) - 1):
        for j, c in creases:
            e = bm.edges.get((verts[r][j], verts[r + 1][j]))
            if e is not None:
                e[crease] = c
    for ring in (verts[0], verts[-1]):
        bm.faces.new(ring).material_index = BODY_MATS.index("paint")
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(mesh)
    bm.free()

    obj = bpy.data.objects.new("body", mesh)
    bpy.context.collection.objects.link(obj)
    for name in BODY_MATS:
        mesh.materials.append(mats[name])
    mirror = obj.modifiers.new("mirror", "MIRROR")
    mirror.use_clip = True
    mirror.use_mirror_merge = True
    mirror.merge_threshold = 0.001
    sub = obj.modifiers.new("subsurf", "SUBSURF")
    sub.levels = SUBDIVISIONS
    sub.render_levels = SUBDIVISIONS
    apply_modifiers(obj)
    return obj


def apply_modifiers(obj):
    bpy.context.view_layer.objects.active = obj
    for m in list(obj.modifiers):
        bpy.ops.object.modifier_apply(modifier=m.name)


def flare_arches(d, body, frame, inset=0.035):
    """Pulls the body sides in away from the wheels, so the arches stand
    proud of the doors and bumpers as flares. The widest point stays at the
    data's overall width."""
    hw = frame.hw
    wells = [(ax["y"], ax["z"], ax["r"] + ARCH_GAP) for ax in axles(d)]

    def smooth(t):
        t = max(0.0, min(1.0, t))
        return t * t * (3 - 2 * t)

    for v in body.data.vertices:
        x, y, z = v.co
        side = abs(x) / hw
        if side < 0.6:
            continue
        near = max(1.0 - smooth((math.hypot(y - ay, z - az) - R - 0.05) / 0.17) for ay, az, R in wells)
        s = frame.y_nose - y
        zb = frame.belt(s)
        zw = wells[0][1] + 0.05
        height = 1.0 - 0.75 * smooth((z - zw) / max(zb - zw, 0.05))
        pull = inset * (1.0 - near) * height * ((side - 0.6) / 0.4) ** 3
        v.co.x = x - math.copysign(pull, x)
    body.data.update()


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
    under = BODY_MATS.index("under")
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


# --- details on the body -------------------------------------------------

class Detailer:
    """Makes the parts drawn on or fixed to the body. Panels are grids cast
    onto the body along a direction and lifted off it, so they follow its
    curves; `pts` are rough points on the surface."""

    def __init__(self, body, mats):
        self.body = body
        self.mats = mats
        self.parts = []
        self.missed = []

    def hit(self, p, direction):
        direction = Vector(direction).normalized()
        depsgraph = bpy.context.evaluated_depsgraph_get()
        ok, loc, normal, _ = self.body.ray_cast(Vector(p) - direction * 1.0, direction, depsgraph=depsgraph)
        return (loc, normal) if ok else (None, None)

    def panel(self, name, mat, pts, direction, nu=None, nv=None, lift=0.004, tile=None):
        direction = Vector(direction).normalized()
        c00, c10, c11, c01 = [Vector(p) for p in pts]
        width = max((c10 - c00).length, (c11 - c01).length)
        height = max((c01 - c00).length, (c11 - c10).length)
        nu = nu or max(2, min(24, int(width / 0.04)))
        nv = nv or max(1, min(12, int(height / 0.04)))
        grid = []
        for v in range(nv + 1):
            row = []
            for u in range(nu + 1):
                a, b = u / nu, v / nv
                p = (c00 * (1 - a) + c10 * a) * (1 - b) + (c01 * (1 - a) + c11 * a) * b
                loc, normal = self.hit(p, direction)
                if loc is None:
                    self.missed.append(name)
                    return None
                row.append((loc + normal * lift, (a * width / tile, b * height / tile) if tile else (a, b)))
            grid.append(row)
        mesh = bpy.data.meshes.new(name)
        bm = bmesh.new()
        uv = bm.loops.layers.uv.new("UVMap")
        vs = [[bm.verts.new(p) for p, _ in row] for row in grid]
        for v in range(nv):
            for u in range(nu):
                corners = ((v, u), (v, u + 1), (v + 1, u + 1), (v + 1, u))
                try:
                    f = bm.faces.new([vs[i][j] for i, j in corners])
                except ValueError:
                    continue
                for loop, (i, j) in zip(f.loops, corners):
                    loop[uv].uv = grid[i][j][1]
        bm.normal_update()
        for f in bm.faces:
            if f.normal.dot(-direction) < 0:
                f.normal_flip()
        bm.to_mesh(mesh)
        bm.free()
        mesh.materials.append(self.mats[mat])
        obj = bpy.data.objects.new(name, mesh)
        bpy.context.collection.objects.link(obj)
        for poly in mesh.polygons:
            poly.use_smooth = True
        self.parts.append(obj)
        return obj

    def ring(self, name, mat, centre, r_out, r_in, direction, lift=0.005, segs=32):
        """A round panel, or a ring when `r_in` > 0, cast onto the body
        around `centre` (a rough point on the surface)."""
        direction = Vector(direction).normalized()
        up = Vector((0.0, 0.0, 1.0)) if abs(direction.z) < 0.9 else Vector((1.0, 0.0, 0.0))
        u = direction.cross(up).normalized()
        v = u.cross(direction).normalized()
        c = Vector(centre)
        radii = [r_in + (r_out - r_in) * k / 2 for k in range(3)] if r_in > 0 else [r_out * k / 3 for k in range(1, 4)]
        rings = []
        for r in radii:
            row = []
            for k in range(segs):
                t = 2 * math.pi * k / segs
                loc, normal = self.hit(c + (u * math.cos(t) + v * math.sin(t)) * r, direction)
                if loc is None:
                    self.missed.append(name)
                    return None
                row.append(loc + normal * lift)
            rings.append(row)
        mesh = bpy.data.meshes.new(name)
        bm = bmesh.new()
        vs = [[bm.verts.new(p) for p in row] for row in rings]
        if r_in <= 0:
            loc, normal = self.hit(c, direction)
            if loc is None:
                self.missed.append(name)
                return None
            mid = bm.verts.new(loc + normal * lift)
            for k in range(segs):
                bm.faces.new((mid, vs[0][k], vs[0][(k + 1) % segs]))
        for a, b in zip(vs, vs[1:]):
            for k in range(segs):
                bm.faces.new((a[k], a[(k + 1) % segs], b[(k + 1) % segs], b[k]))
        bm.normal_update()
        for f in bm.faces:
            if f.normal.dot(-direction) < 0:
                f.normal_flip()
        bm.to_mesh(mesh)
        bm.free()
        mesh.materials.append(self.mats[mat])
        obj = bpy.data.objects.new(name, mesh)
        bpy.context.collection.objects.link(obj)
        for poly in mesh.polygons:
            poly.use_smooth = True
        self.parts.append(obj)
        return obj

    def line(self, name, pts, direction, width=0.006, mat="seam", lift=0.003):
        """A thin strip along a polyline: a panel gap or a pinstripe."""
        direction = Vector(direction).normalized()
        for a, b in zip(pts, pts[1:]):
            a, b = Vector(a), Vector(b)
            perp = (b - a).cross(direction).normalized() * (0.5 * width)
            n = max(2, int((b - a).length / 0.04))
            self.panel(name, mat, [a - perp, b - perp, b + perp, a + perp], direction, nu=n, nv=1, lift=lift)

    def solid(self, name, mat, size, location, bevel=0.01, rotation=(0.0, 0.0, 0.0)):
        bpy.ops.mesh.primitive_cube_add(size=1.0, location=location, rotation=rotation)
        o = bpy.context.active_object
        o.name = name
        o.scale = size
        bpy.ops.object.transform_apply(scale=True)
        if bevel > 0:
            m = o.modifiers.new("bevel", "BEVEL")
            m.width = bevel
            m.segments = 3
            apply_modifiers(o)
        o.data.materials.append(self.mats[mat])
        for poly in o.data.polygons:
            poly.use_smooth = True
        self.parts.append(o)
        return o

    def cylinder(self, name, mat, radius, depth, location, rotation, vertices=24):
        bpy.ops.mesh.primitive_cylinder_add(vertices=vertices, radius=radius, depth=depth,
                                            location=location, rotation=rotation)
        o = bpy.context.active_object
        o.name = name
        o.data.materials.append(self.mats[mat])
        for poly in o.data.polygons:
            poly.use_smooth = len(poly.vertices) == 4
        self.parts.append(o)
        return o

    def extrude_x(self, name, mat, profile_at, xs):
        """A part swept across the car: `profile_at(x)` gives a closed (y, z)
        loop for each x in `xs`."""
        mesh = bpy.data.meshes.new(name)
        bm = bmesh.new()
        loops = [[bm.verts.new((x, y, z)) for y, z in profile_at(x)] for x in xs]
        n = len(loops[0])
        for a, b in zip(loops, loops[1:]):
            for k in range(n):
                bm.faces.new((a[k], a[(k + 1) % n], b[(k + 1) % n], b[k]))
        bm.faces.new(loops[0])
        bm.faces.new(list(reversed(loops[-1])))
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        bm.to_mesh(mesh)
        bm.free()
        mesh.materials.append(self.mats[mat])
        obj = bpy.data.objects.new(name, mesh)
        bpy.context.collection.objects.link(obj)
        self.parts.append(obj)
        return obj


def expand(quad, ds, df):
    """Grows a window outline given as (s, f) corners."""
    cs = sum(p[0] for p in quad) / 4
    cf = sum(p[1] for p in quad) / 4
    return [(s + math.copysign(ds, s - cs), f + math.copysign(df, f - cf)) for s, f in quad]


def add_details(d, body, mats, frame, stations, places):
    det = Detailer(body, mats)
    hw, L = frame.hw, d["length"]
    y_nose, y_tail = frame.y(0.0), frame.y(L)
    sedan = d["style"] == "sedan"
    ax = axles(d)
    R = [a["r"] + ARCH_GAP for a in ax]
    fa, ra, b = places["fa"], places["ra"], places["b_pillar"]
    rt = places["rear"][0]
    sb, st_ = places["screen"]
    rt, rb = places["rear"]
    front_belt = frame.belt(0.05)
    rear_belt = frame.belt(L - 0.06)

    for side in (1.0, -1.0):
        sdir = (-side, 0.0, 0.0)

        def wp(s, f):
            return frame.window_point(s, f, side)

        # Side glass on black surrounds; the B-pillar between the doors is
        # black too.
        wins = places["windows"]
        for k, quad in enumerate(wins):
            det.panel("window_surround", "gloss_black", [wp(*p) for p in expand(quad, 0.025, 0.05)], sdir, lift=0.003)
            det.panel("side_glass", "glass", [wp(*p) for p in quad], sdir, lift=0.006)
        det.panel("b_pillar", "gloss_black", [wp(b - 0.05, -0.01), wp(b + 0.05, -0.01), wp(b + 0.05, 0.97), wp(b - 0.05, 0.97)], sdir, lift=0.003)

        # Door shut lines: front door ahead of the B-pillar, rear door
        # behind it, curving round the rear arch.
        z_sill = frame.zf + 0.06
        s_fd = fa + R[0] + 0.10
        det.line("door_gap", [(side * hw, frame.y(s_fd), z_sill), (side * hw, frame.y(s_fd), frame.belt(s_fd) - 0.01),
                              (side * hw, frame.y(sb + 0.11), frame.belt(sb + 0.11) + 0.005)], sdir)
        det.line("door_gap", [(side * hw, frame.y(b), z_sill), (side * hw, frame.y(b), frame.belt(b) - 0.005)], sdir)
        # Rear door's rear edge: up from the sill, round the front of the
        # rear arch (forward is +y), then up to the belt.
        q = places["rear_door"]
        if q is not None:
            ra_ = R[1] + 0.05
            arch = [(side * hw, ax[1]["y"] + ra_, z_sill)]
            for k in range(7):
                t = math.radians(max(math.degrees(math.asin(max(-1.0, min(1.0, (z_sill - ax[1]["z"]) / ra_)))), 0.0) + 11 * k)
                arch.append((side * hw, ax[1]["y"] + ra_ * math.cos(t), ax[1]["z"] + ra_ * math.sin(t)))
            arch.append((side * hw, frame.y(q + 0.02), frame.belt(q + 0.02) - 0.01))
            det.line("door_gap", arch, sdir)
        if places["floating_roof"]:
            # Black arch trims round both wheels, on the flares: strips cast
            # in several rows so they follow the flare's curve.
            for i in range(2):
                r0, r1 = R[i] + 0.004, R[i] + 0.072
                for k in range(24):
                    t0, t1 = (math.radians(-12 + 204 * (k + e) / 24) for e in (0, 1))
                    if min(ax[i]["z"] + r1 * math.sin(t) for t in (t0, t1)) < frame.zf + 0.01:
                        continue

                    def at(r, t):
                        return (side * hw, ax[i]["y"] + r * math.cos(t), ax[i]["z"] + r * math.sin(t))
                    det.panel("arch_trim", "trim", [at(r0, t0), at(r0, t1), at(r1, t1), at(r1, t0)], sdir, nu=3, nv=4, lift=0.004)

        # Door handles: body-coloured pulls on the hatch, flush bars on
        # the sedan.
        for s in ((b - 0.26,) if q is None else (b - 0.32, q - 0.30)):
            z = frame.belt(s) - 0.075
            loc, n = det.hit((side * hw, frame.y(s), z), sdir)
            if loc is None:
                continue
            if sedan:
                det.panel("handle", "chrome", [(side * hw, frame.y(s - 0.08), z - 0.013), (side * hw, frame.y(s + 0.08), z - 0.013),
                                               (side * hw, frame.y(s + 0.08), z + 0.013), (side * hw, frame.y(s - 0.08), z + 0.013)], sdir, lift=0.004)
            else:
                det.panel("handle_recess", "trim", [(side * hw, frame.y(s - 0.08), z - 0.025), (side * hw, frame.y(s + 0.08), z - 0.025),
                                                    (side * hw, frame.y(s + 0.08), z + 0.02), (side * hw, frame.y(s - 0.08), z + 0.02)], sdir, lift=0.002)
                det.solid("handle", "paint", (0.022, 0.15, 0.028), loc + n * 0.016, bevel=0.009)

        # Door mirror on a short arm at the front of the side glass.
        s_m = sb + 0.17
        zm = frame.belt(s_m) + 0.06
        loc, n = det.hit((side * hw, frame.y(s_m), zm), sdir)
        if loc is not None:
            centre = loc + Vector((side * 0.13, -0.01, 0.03))
            det.solid("mirror_arm", "trim" if not sedan else "paint", (0.10, 0.05, 0.03), loc + Vector((side * 0.05, 0.0, 0.0)), bevel=0.008)
            det.solid("mirror", "roof" if places["floating_roof"] else "paint", (0.21, 0.09, 0.115), centre, bevel=0.035,
                      rotation=(0.0, 0.0, side * math.radians(-6)))
            det.solid("mirror_glass", "housing", (0.18, 0.01, 0.09), centre + Vector((0.0, -0.046, 0.0)), bevel=0.0,
                      rotation=(0.0, 0.0, side * math.radians(-6)))

        # Sill: black on the hatch, a dark side skirt on the sedan.
        y0 = ax[0]["y"] - R[0] - 0.02
        y1 = ax[1]["y"] + R[1] + 0.02
        det.panel("sill", "trim", [(side * hw, y0, frame.zf + 0.02), (side * hw, y1, frame.zf + 0.02),
                                   (side * hw, y1, frame.zf + 0.10), (side * hw, y0, frame.zf + 0.10)], sdir, nv=2)

    if sedan:
        add_sedan_ends(det, d, frame, places, front_belt, rear_belt)
    elif d["style"] == "small":
        add_small_ends(det, d, frame, places, front_belt, rear_belt)
    else:
        add_hatch_ends(det, d, frame, places, front_belt, rear_belt)
    if det.missed:
        print("QTS: parts that missed the body:", sorted(set(det.missed)))
    return det.parts


def add_hatch_ends(det, d, frame, places, front_belt, rear_belt):
    hw, L = frame.hw, d["length"]
    fwd, back, down = (0.0, -1.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, -1.0)
    yn, yt = frame.y(0.0), frame.y(L)
    zl = front_belt - 0.085                  # lamp line
    # Slim headlamps joined by a black grille band with a thin red line
    # through it.
    for side in (1.0, -1.0):
        x0, x1 = side * 0.40 * hw, side * 0.94 * hw
        det.panel("headlamp_housing", "housing", [(x0, yn, zl - 0.04), (x1, yn, zl - 0.02), (x1, yn, zl + 0.085), (x0, yn, zl + 0.07)], fwd)
        # Light strip along the bottom, turning up the outer end.
        det.line("headlamp_drl", [(side * 0.44 * hw, yn, zl - 0.028), (side * 0.90 * hw, yn, zl - 0.01), (side * 0.91 * hw, yn, zl + 0.06)],
                 fwd, width=0.012, mat="head", lift=0.007)
        for xc in (0.57, 0.75):
            x = side * xc * hw
            det.panel("headlamp_lens", "lens", [(x - 0.04, yn, zl + 0.0), (x + 0.04, yn, zl + 0.004),
                                                (x + 0.04, yn, zl + 0.058), (x - 0.04, yn, zl + 0.054)], fwd, lift=0.007)
    det.panel("grille", "gloss_black", [(-0.40 * hw, yn, zl - 0.035), (0.40 * hw, yn, zl - 0.035), (0.40 * hw, yn, zl + 0.05), (-0.40 * hw, yn, zl + 0.05)], fwd)
    det.panel("grille_mesh", "honeycomb", [(-0.37 * hw, yn, zl - 0.02), (0.37 * hw, yn, zl - 0.02), (0.37 * hw, yn, zl + 0.035), (-0.37 * hw, yn, zl + 0.035)],
              fwd, lift=0.006, tile=0.06)
    det.line("grille_accent", [(-0.88 * hw, yn, zl - 0.042), (-0.40 * hw, yn, zl - 0.044), (0.40 * hw, yn, zl - 0.044), (0.88 * hw, yn, zl - 0.042)],
             fwd, width=0.008, mat="accent", lift=0.008)
    # Big lower intake with a mesh, and side intakes with fog lamps.
    zi = frame.lift(0.05) + 0.06
    det.panel("intake", "trim", [(-0.60 * hw, yn, zi - 0.01), (0.60 * hw, yn, zi - 0.01), (0.52 * hw, yn, zi + 0.17), (-0.52 * hw, yn, zi + 0.17)], fwd)
    det.panel("intake_mesh", "honeycomb", [(-0.56 * hw, yn, zi + 0.01), (0.56 * hw, yn, zi + 0.01), (0.49 * hw, yn, zi + 0.15), (-0.49 * hw, yn, zi + 0.15)],
              fwd, lift=0.006, tile=0.06)
    for side in (1.0, -1.0):
        x0, x1 = side * 0.70 * hw, side * 0.88 * hw
        det.panel("side_intake", "honeycomb", [(x0, yn, zi + 0.02), (x1, yn, zi + 0.04), (x1, yn, zi + 0.16), (x0, yn, zi + 0.16)], fwd, tile=0.06)
        det.panel("fog_lamp", "lens", [(side * 0.74 * hw, yn, zi + 0.12), (side * 0.84 * hw, yn, zi + 0.12),
                                       (side * 0.84 * hw, yn, zi + 0.15), (side * 0.74 * hw, yn, zi + 0.15)], fwd, lift=0.007)
    det.panel("front_plate", "plate", [(-0.24, yn, zi + 0.04), (0.24, yn, zi + 0.04), (0.24, yn, zi + 0.15), (-0.24, yn, zi + 0.15)], fwd, lift=0.009)
    # Bonnet shut lines, seen from above.
    sb = places["screen"][0]
    for side in (1.0, -1.0):
        det.line("bonnet_gap", [(side * 0.80 * hw, frame.y(0.10), frame.roof(0.10)), (side * 0.84 * hw, frame.y(sb - 0.04), frame.roof(sb - 0.04))], down)
    det.line("bonnet_gap", [(-0.80 * hw, frame.y(0.10), frame.roof(0.10)), (0.80 * hw, frame.y(0.10), frame.roof(0.10))], down)

    # Roof spoiler overhanging the tailgate glass.
    rt, rb = places["rear"]
    zr = frame.roof(rt)

    def spoiler(x):
        loc, _ = det.hit((x, frame.y(rt - 0.22), zr), down)
        z0 = loc.z if loc is not None else zr
        return [(frame.y(rt - 0.24), z0 - 0.005), (frame.y(rt - 0.04), z0 + 0.012), (frame.y(rt + 0.10), z0 - 0.015),
                (frame.y(rt + 0.10), z0 - 0.045), (frame.y(rt - 0.06), z0 - 0.04)]

    span = 0.70 * hw
    det.extrude_x("roof_spoiler", "paint", spoiler, [span * (2 * k / 10 - 1) for k in range(11)])
    det.panel("brake_light", "tail_bright", [(-0.12, frame.y(rt + 0.10), zr - 0.04), (0.12, frame.y(rt + 0.10), zr - 0.04),
                                             (0.12, frame.y(rt + 0.10), zr - 0.025), (-0.12, frame.y(rt + 0.10), zr - 0.025)], back, lift=0.0)
    # Rear wiper on the tailgate glass.
    zw = frame.roof(rb) + 0.03
    det.line("wiper", [(0.0, yt, zw), (-0.32 * hw, yt, zw + 0.10)], back, width=0.012, mat="trim", lift=0.01)

    # Tail lamps across the corners, split by the tailgate gap.
    zt = rear_belt - 0.13
    for side in (1.0, -1.0):
        x0, x1 = side * 0.42 * hw, side * 0.82 * hw
        det.panel("taillamp", "tail", [(x0, yt, zt), (x1, yt, zt - 0.005), (x1, yt, zt + 0.105), (x0, yt, zt + 0.08)], back)
        det.panel("taillamp_bar", "tail_bright", [(side * 0.48 * hw, yt, zt + 0.03), (side * 0.79 * hw, yt, zt + 0.028),
                                                  (side * 0.79 * hw, yt, zt + 0.05), (side * 0.48 * hw, yt, zt + 0.045)], back, lift=0.007)
        det.line("tailgate_gap", [(side * 0.60 * hw, yt, rear_belt - 0.31), (side * 0.60 * hw, yt, zt - 0.005), (side * 0.60 * hw, yt, zt + 0.10)],
                 back, lift=0.009)
        # Twin round tailpipes, one each side.
        zp = frame.lift(L - 0.06) + 0.06
        loc, _ = det.hit((side * 0.62 * hw, yt, zp), back)
        if loc is not None:
            det.cylinder("tailpipe", "chrome", 0.042, 0.09, (side * 0.62 * hw, loc.y - 0.02, zp), (math.pi / 2, 0.0, 0.0))
            det.cylinder("tailpipe_bore", "under", 0.034, 0.092, (side * 0.62 * hw, loc.y - 0.02, zp), (math.pi / 2, 0.0, 0.0))
    det.line("tailgate_gap", [(-0.60 * hw, yt, rear_belt - 0.31), (0.60 * hw, yt, rear_belt - 0.31)], back, lift=0.009)
    det.panel("rear_plate", "plate", [(-0.24, yt, rear_belt - 0.29), (0.24, yt, rear_belt - 0.29), (0.24, yt, rear_belt - 0.18), (-0.24, yt, rear_belt - 0.18)],
              back, lift=0.008)
    zv = frame.lift(L - 0.06) + 0.02
    det.panel("diffuser", "trim", [(-0.85 * hw, yt, zv), (0.85 * hw, yt, zv), (0.85 * hw, yt, zv + 0.12), (-0.85 * hw, yt, zv + 0.12)], back)


def add_small_ends(det, d, frame, places, front_belt, rear_belt):
    """Upright face with round headlamps set in the corners of a bonnet
    that wraps down the sides; a small grille and a wide lower intake in
    the bumper below it; a scoop on the bonnet; upright tail lamps at the
    corners, twin pipes in the middle."""
    hw, L = frame.hw, d["length"]
    fwd, back, down = (0.0, -1.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, -1.0)
    yn, yt = frame.y(0.0), frame.y(L)
    zf = frame.zf
    zb = front_belt
    # Heights on the face, from the floor line.
    z_lamp = zb - 0.095
    z_shut = zb - 0.225
    for side in (1.0, -1.0):
        c = (side * 0.58 * hw, yn, z_lamp)
        det.ring("headlamp_bezel", "chrome", c, 0.122, 0.106, fwd, lift=0.006)
        det.ring("headlamp", "housing", c, 0.106, 0.0, fwd, lift=0.005)
        det.ring("headlamp_drl", "head", c, 0.095, 0.078, fwd, lift=0.008)
        det.ring("headlamp_lens", "lens", c, 0.038, 0.0, fwd, lift=0.009)
        det.ring("indicator", "amber", (side * 0.86 * hw, yn, z_shut - 0.05), 0.022, 0.0, fwd, lift=0.006)
    # Bonnet shut line: across the face under the lamps, then back along
    # each side just under the belt to the screen.
    sb = places["screen"][0]
    det.line("bonnet_gap", [(-0.93 * hw, yn, z_shut), (0.93 * hw, yn, z_shut)], fwd, lift=0.004)
    for side in (1.0, -1.0):
        det.line("bonnet_gap", [(side * hw, frame.y(s), frame.belt(s) - 0.03) for s in (0.07, 0.3, 0.6, sb - 0.03)], (-side, 0.0, 0.0))
    # Grille under the shut line, with chrome bars.
    g0, g1 = z_shut - 0.135, z_shut - 0.015
    det.panel("grille", "gloss_black", [(-0.38 * hw, yn, g0), (0.38 * hw, yn, g0), (0.43 * hw, yn, g1), (-0.43 * hw, yn, g1)], fwd)
    det.line("grille_surround", [(-0.38 * hw, yn, g0), (0.38 * hw, yn, g0), (0.43 * hw, yn, g1), (-0.43 * hw, yn, g1), (-0.38 * hw, yn, g0)],
             fwd, width=0.012, mat="chrome", lift=0.007)
    for k in (1, 2, 3):
        z = g0 + (g1 - g0) * k / 4
        det.line("grille_bar", [(-0.39 * hw, yn, z), (0.39 * hw, yn, z)], fwd, width=0.008, mat="chrome", lift=0.007)
    det.panel("front_plate", "plate", [(-0.24, yn, g0 - 0.125), (0.24, yn, g0 - 0.125), (0.24, yn, g0 - 0.015), (-0.24, yn, g0 - 0.015)], fwd, lift=0.008)
    # Wide lower intake with a mesh and round fog lamps at its ends.
    zi = frame.lift(0.05) + 0.03
    det.panel("intake", "trim", [(-0.80 * hw, yn, zi), (0.80 * hw, yn, zi), (0.74 * hw, yn, zi + 0.13), (-0.74 * hw, yn, zi + 0.13)], fwd)
    det.panel("intake_mesh", "honeycomb", [(-0.55 * hw, yn, zi + 0.015), (0.55 * hw, yn, zi + 0.015), (0.52 * hw, yn, zi + 0.115), (-0.52 * hw, yn, zi + 0.115)],
              fwd, lift=0.006, tile=0.06)
    for side in (1.0, -1.0):
        det.ring("fog_lamp", "lens", (side * 0.68 * hw, yn, zi + 0.065), 0.038, 0.0, fwd, lift=0.007)
    # Bonnet scoop.
    s0, s1 = 0.30, 0.56

    def scoop(x):
        loc0, _ = det.hit((x, frame.y(s0), frame.roof(s0)), down)
        loc1, _ = det.hit((x, frame.y(s1), frame.roof(s1)), down)
        z0 = loc0.z if loc0 is not None else frame.roof(s0)
        z1 = loc1.z if loc1 is not None else frame.roof(s1)
        return [(frame.y(s0), z0 + 0.045), (frame.y(s1), z1 + 0.004), (frame.y(s1), z1 - 0.02), (frame.y(s0), z0 - 0.02)]

    det.extrude_x("bonnet_scoop", "paint", scoop, [0.17 * (2 * k / 6 - 1) for k in range(7)])
    loc, _ = det.hit((0.0, frame.y(s0), frame.roof(s0)), down)
    if loc is not None:
        det.solid("scoop_mouth", "gloss_black", (0.30, 0.006, 0.034), (0.0, frame.y(s0) + 0.001, loc.z + 0.018), bevel=0.0)

    # Rear: a small roof spoiler in the roof colour, upright corner lamps,
    # tailgate with the plate, twin pipes in the middle.
    rt, rb = places["rear"]
    zr = frame.roof(rt)

    def spoiler(x):
        loc, _ = det.hit((x, frame.y(rt - 0.12), zr), down)
        z0 = loc.z if loc is not None else zr
        return [(frame.y(rt - 0.16), z0 - 0.004), (frame.y(rt - 0.02), z0 + 0.010), (frame.y(rt + 0.07), z0 - 0.012),
                (frame.y(rt + 0.07), z0 - 0.035), (frame.y(rt - 0.04), z0 - 0.03)]

    det.extrude_x("roof_spoiler", "roof", spoiler, [0.66 * hw * (2 * k / 10 - 1) for k in range(11)])
    det.panel("brake_light", "tail_bright", [(-0.10, frame.y(rt + 0.07), zr - 0.03), (0.10, frame.y(rt + 0.07), zr - 0.03),
                                             (0.10, frame.y(rt + 0.07), zr - 0.02), (-0.10, frame.y(rt + 0.07), zr - 0.02)], back, lift=0.0)
    zw = frame.roof(rb) + 0.04
    det.line("wiper", [(0.0, yt, zw), (-0.30 * hw, yt, zw + 0.08)], back, width=0.012, mat="trim", lift=0.01)
    zt0, zt1 = rear_belt - 0.27, rear_belt - 0.02
    for side in (1.0, -1.0):
        x0, x1 = side * 0.57 * hw, side * 0.77 * hw
        det.panel("taillamp_bezel", "chrome", [(x0 - side * 0.012, yt, zt0 - 0.012), (x1 + side * 0.012, yt, zt0 - 0.012),
                                               (x1 + side * 0.012, yt, zt1 + 0.012), (x0 - side * 0.012, yt, zt1 + 0.012)], back, lift=0.004)
        det.panel("taillamp", "tail", [(x0, yt, zt0), (x1, yt, zt0), (x1, yt, zt1), (x0, yt, zt1)], back, lift=0.006)
        det.line("taillamp_ring", [(x0 + side * 0.03, yt, zt0 + 0.03), (x1 - side * 0.03, yt, zt0 + 0.03), (x1 - side * 0.03, yt, zt1 - 0.03),
                                   (x0 + side * 0.03, yt, zt1 - 0.03), (x0 + side * 0.03, yt, zt0 + 0.03)], back, width=0.014, mat="tail_bright", lift=0.008)
        det.line("tailgate_gap", [(side * 0.51 * hw, yt, rear_belt - 0.36), (side * 0.51 * hw, yt, rear_belt + 0.03)], back, lift=0.009)
        det.panel("reflector", "tail", [(side * 0.56 * hw, yt, frame.lift(L - 0.02) + 0.10), (side * 0.76 * hw, yt, frame.lift(L - 0.02) + 0.10),
                                        (side * 0.76 * hw, yt, frame.lift(L - 0.02) + 0.13), (side * 0.56 * hw, yt, frame.lift(L - 0.02) + 0.13)], back)
    det.line("tailgate_gap", [(-0.51 * hw, yt, rear_belt - 0.36), (0.51 * hw, yt, rear_belt - 0.36)], back, lift=0.009)
    det.panel("rear_plate", "plate", [(-0.24, yt, rear_belt - 0.33), (0.24, yt, rear_belt - 0.33), (0.24, yt, rear_belt - 0.22), (-0.24, yt, rear_belt - 0.22)],
              back, lift=0.008)
    det.line("tailgate_handle", [(-0.16, yt, rear_belt - 0.19), (0.16, yt, rear_belt - 0.19)], back, width=0.02, mat="chrome", lift=0.008)
    zv = frame.lift(L - 0.02) + 0.01
    det.panel("valance", "trim", [(-0.74 * hw, yt, zv), (0.74 * hw, yt, zv), (0.74 * hw, yt, zv + 0.09), (-0.74 * hw, yt, zv + 0.09)], back)
    loc, _ = det.hit((0.0, yt, zv + 0.045), back)
    if loc is not None:
        for x in (-0.075, 0.075):
            det.cylinder("tailpipe", "chrome", 0.036, 0.09, (x, loc.y - 0.02, zv + 0.045), (math.pi / 2, 0.0, 0.0))
            det.cylinder("tailpipe_bore", "under", 0.029, 0.092, (x, loc.y - 0.02, zv + 0.045), (math.pi / 2, 0.0, 0.0))


def add_sedan_ends(det, d, frame, places, front_belt, rear_belt):
    hw, L = frame.hw, d["length"]
    fwd, back, down = (0.0, -1.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, -1.0)
    nose_top = (0.0, -1.0, -0.55)
    yn, yt = frame.y(0.0), frame.y(L)
    # Swept headlamps set into the top of the rounded nose; no upper grille.
    zl = front_belt - 0.02
    for side in (1.0, -1.0):
        x0, x1 = side * 0.48 * hw, side * 0.95 * hw
        pts = [(x0, frame.y(0.03), zl - 0.05), (x1, frame.y(0.12), zl - 0.04), (x1, frame.y(0.22), zl + 0.04), (side * 0.56 * hw, frame.y(0.06), zl + 0.035)]
        det.panel("headlamp_housing", "housing", pts, nose_top)
        det.line("headlamp_drl", [(side * 0.57 * hw, frame.y(0.06), zl + 0.022), (side * 0.93 * hw, frame.y(0.21), zl + 0.03)],
                 nose_top, width=0.012, mat="head", lift=0.007)
        for xc in (0.68, 0.83):
            x = side * xc * hw
            det.panel("headlamp_lens", "lens", [(x - 0.04, frame.y(0.07), zl - 0.03), (x + 0.04, frame.y(0.10), zl - 0.03),
                                                (x + 0.04, frame.y(0.10), zl + 0.01), (x - 0.04, frame.y(0.07), zl + 0.01)], nose_top, lift=0.007)
    # A slim lower intake across the bumper, with corner vents.
    zi = frame.lift(0.05) + 0.07
    det.panel("intake", "trim", [(-0.55 * hw, yn, zi), (0.55 * hw, yn, zi), (0.50 * hw, yn, zi + 0.075), (-0.50 * hw, yn, zi + 0.075)], fwd)
    for side in (1.0, -1.0):
        det.panel("corner_vent", "trim", [(side * 0.70 * hw, yn, zi + 0.01), (side * 0.86 * hw, yn, zi + 0.03),
                                          (side * 0.86 * hw, yn, zi + 0.14), (side * 0.74 * hw, yn, zi + 0.12)], fwd)
    det.line("bonnet_gap", [(-0.78 * hw, frame.y(0.30), frame.roof(0.30)), (0.78 * hw, frame.y(0.30), frame.roof(0.30))], down)
    sb = places["screen"][0]
    for side in (1.0, -1.0):
        det.line("bonnet_gap", [(side * 0.78 * hw, frame.y(0.30), frame.roof(0.30)), (side * 0.82 * hw, frame.y(sb - 0.05), frame.roof(sb - 0.05))], down)

    # Boot lid lines on the deck, and a small dark lip spoiler at its edge.
    rt, rb = places["rear"]
    s_lip = L - 0.30
    for side in (1.0, -1.0):
        det.line("boot_gap", [(side * 0.80 * hw, frame.y(rb + 0.03), frame.roof(rb + 0.03)), (side * 0.80 * hw, frame.y(s_lip), frame.roof(s_lip))], down)
    det.line("boot_gap", [(-0.80 * hw, frame.y(rb + 0.03), frame.roof(rb + 0.03)), (0.80 * hw, frame.y(rb + 0.03), frame.roof(rb + 0.03))], down)

    def lip(x):
        loc, _ = det.hit((x, frame.y(s_lip - 0.02), frame.roof(s_lip)), down)
        z0 = loc.z if loc is not None else frame.roof(s_lip)
        return [(frame.y(s_lip - 0.08), z0 - 0.003), (frame.y(s_lip + 0.04), z0 + 0.022), (frame.y(s_lip + 0.05), z0 + 0.012), (frame.y(s_lip - 0.02), z0 - 0.01)]

    det.extrude_x("lip_spoiler", "gloss_black", lip, [0.80 * hw * (2 * k / 10 - 1) for k in range(11)])

    # Tail lamps that wrap from the back round onto the sides.
    zt = rear_belt - 0.10
    for side in (1.0, -1.0):
        x0, x1 = side * 0.40 * hw, side * 0.83 * hw
        det.panel("taillamp", "tail", [(x0, yt, zt + 0.01), (x1, yt, zt - 0.01), (x1, yt, zt + 0.085), (x0, yt, zt + 0.075)], back)
        det.panel("taillamp_bar", "tail_bright", [(side * 0.45 * hw, yt, zt + 0.055), (side * 0.80 * hw, yt, zt + 0.05),
                                                  (side * 0.80 * hw, yt, zt + 0.065), (side * 0.45 * hw, yt, zt + 0.07)], back, lift=0.007)
        sdir = (-side, 0.0, 0.0)
        det.panel("taillamp_side", "tail", [(side * hw, frame.y(L - 0.32), zt + 0.035), (side * hw, frame.y(L - 0.13), zt + 0.005),
                                            (side * hw, frame.y(L - 0.13), zt + 0.08), (side * hw, frame.y(L - 0.30), zt + 0.08)], sdir)
    det.line("boot_gap", [(-0.40 * hw, yt, zt + 0.005), (-0.40 * hw, yt, zt - 0.08), (0.40 * hw, yt, zt - 0.08), (0.40 * hw, yt, zt + 0.005)], back, lift=0.009)
    det.panel("rear_plate", "plate", [(-0.24, yt, zt - 0.22), (0.24, yt, zt - 0.22), (0.24, yt, zt - 0.11), (-0.24, yt, zt - 0.11)], back, lift=0.008)
    zv = frame.lift(L - 0.06) + 0.01
    det.panel("diffuser", "trim", [(-0.80 * hw, yt, zv), (0.80 * hw, yt, zv), (0.75 * hw, yt, zv + 0.13), (-0.75 * hw, yt, zv + 0.13)], back)
    for k in range(-2, 3):
        x = k * 0.16 * hw
        det.line("diffuser_fin", [(x, yt, zv + 0.005), (x, yt, zv + 0.11)], back, width=0.012, mat="gloss_black", lift=0.008)


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


def join(parts, name):
    bpy.ops.object.select_all(action="DESELECT")
    for p in parts:
        p.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.join()
    o = bpy.context.active_object
    o.name = name
    o.data.name = name
    o.select_set(False)
    return o


def rim_radius(radius, width):
    # Nominal sizes: the rim is what is left inside the sidewall.
    return radius - 0.40 * width if radius - 0.40 * width > 0.15 else radius * 0.7


def build_wheel(name, radius, width, mats, style):
    """One wheel, axle along X, outer face toward +X, centred on the origin."""
    rim_r = rim_radius(radius, width)
    hw = 0.5 * width
    side = rim_r + 0.25 * (radius - rim_r)
    tire = [
        (rim_r - 0.005, -hw * 0.88), (side, -hw * 1.0),
        (rim_r + 0.75 * (radius - rim_r), -hw * 0.98), (radius - 0.012, -hw * 0.86),
        (radius, -hw * 0.6), (radius, hw * 0.6), (radius - 0.012, hw * 0.86),
        (rim_r + 0.75 * (radius - rim_r), hw * 0.98), (side, hw * 1.0),
        (rim_r - 0.005, hw * 0.88),
    ]
    parts = [lathe("tire", list(reversed(tire)), mats["rubber"], 64)]
    # Tread: shallow circumferential grooves.
    for xg in (-0.3, 0.0, 0.3):
        parts.append(lathe("groove", [(radius + 0.0005, hw * (xg - 0.04)), (radius + 0.0005, hw * (xg + 0.04))], mats["under"], 64))
    lip = [(rim_r - 0.012, -hw * 0.86), (rim_r, -hw * 0.9), (rim_r + 0.008, -hw * 0.9),
           (rim_r + 0.008, hw * 0.9), (rim_r, hw * 0.92), (rim_r - 0.022, hw * 0.84)]
    parts.append(lathe("rim", lip, mats["alloy"], 64))
    hub = [(0.0, hw * 0.62), (0.07 * radius, hw * 0.64), (0.17 * radius, hw * 0.58)]
    parts.append(lathe("hub", hub, mats["alloy"], 32))
    # Wheel nuts round the hub.
    for k in range(5):
        t = 2 * math.pi * k / 5
        r = 0.11 * radius
        bpy.ops.mesh.primitive_cylinder_add(vertices=6, radius=0.011, depth=0.02,
                                            location=(hw * 0.65, r * math.cos(t), r * math.sin(t)), rotation=(0.0, math.pi / 2, 0.0))
        nut = bpy.context.active_object
        nut.data.materials.append(mats["chrome"])
        parts.append(nut)
    disc_r = rim_r - 0.035
    disc = [(0.35 * disc_r, hw * 0.05), (disc_r, hw * 0.05), (disc_r, -hw * 0.2), (0.35 * disc_r, -hw * 0.2)]
    parts.append(lathe("disc", disc, mats["disc"], 48))
    # Spokes: five split pairs on the hatch, ten curved blades on the sedan.
    n, twist, sw = (10, 0.0, 0.045) if style != "sedan" else (10, 0.45, 0.032)
    for k in range(n):
        if style != "sedan":
            t = 2 * math.pi * (k // 2) / 5 + (0.17 if k % 2 else -0.17)
        else:
            t = 2 * math.pi * k / n
        bpy.ops.mesh.primitive_cube_add(size=1.0)
        s = bpy.context.active_object
        length = rim_r - 0.15 * radius
        s.scale = (0.045, sw, length)
        bpy.ops.object.transform_apply(scale=True)
        me = s.data
        sub = bmesh.new()
        sub.from_mesh(me)
        bmesh.ops.subdivide_edges(sub, edges=[e for e in sub.edges if abs(e.verts[0].co.z - e.verts[1].co.z) > 1e-6], cuts=4)
        sub.to_mesh(me)
        sub.free()
        for v in me.vertices:
            u = (v.co.z + 0.5 * length) / length
            v.co.y *= 1.0 - 0.35 * u
            v.co.x += hw * (0.60 - 0.28 * u)
            r = v.co.z + 0.15 * radius + 0.5 * length
            a = t + twist * u
            y0 = v.co.y
            v.co.y = y0 * math.cos(a) - r * math.sin(a)
            v.co.z = y0 * math.sin(a) + r * math.cos(a)
        me.materials.append(mats["alloy"])
        parts.append(s)
    return join(parts, name)


def build_caliper(name, radius, width, mats):
    """A brake caliper over the top of the disc, slightly behind the axle.
    It steers with the wheel but does not spin (car_view.gd)."""
    rim_r = rim_radius(radius, width)
    hw = 0.5 * width
    r0, r1 = rim_r - 0.105, rim_r - 0.018
    x0, x1 = -hw * 0.32, hw * 0.24
    mesh = bpy.data.meshes.new(name)
    bm = bmesh.new()
    loops = []
    steps = 8
    for k in range(steps + 1):
        a = math.radians(70 + 50 * k / steps)        # from the top toward the back
        ring = []
        for x, r in ((x0, r0), (x1, r0), (x1, r1), (x0, r1)):
            ring.append(bm.verts.new((x, r * math.cos(a), r * math.sin(a))))
        loops.append(ring)
    for a, b in zip(loops, loops[1:]):
        for k in range(4):
            bm.faces.new((a[k], a[(k + 1) % 4], b[(k + 1) % 4], b[k]))
    bm.faces.new(loops[0])
    bm.faces.new(list(reversed(loops[-1])))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(mesh)
    bm.free()
    mesh.materials.append(mats["caliper"])
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    bev = obj.modifiers.new("bevel", "BEVEL")
    bev.width = 0.006
    bev.segments = 2
    apply_modifiers(obj)
    return obj


# --- scene -------------------------------------------------------------

def preview(path, d, wheels, calipers):
    """Renders the car from three angles: `path` with -front, -side and
    -rear before the extension."""
    scene = bpy.context.scene
    placed = []
    for ax_i, ax in enumerate(axles(d)):
        for side in (1.0, -1.0):
            for template in (wheels[ax_i], calipers[ax_i]):
                w = template.copy()
                w.location = (side * 0.5 * ax["track"], ax["y"], ax["z"])
                w.scale = (side, 1.0, 1.0)
                bpy.context.collection.objects.link(w)
                placed.append(w)
    for w in wheels + calipers:
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
        scene.render.image_settings.quality = 88
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
    for w in wheels + calipers:
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
    mats = make_materials(d["style"])
    stations, places = body_design(d)
    frame = Frame(d, stations)
    body = build_body(d, mats, frame, stations, places)
    if d["style"] == "small":
        flare_arches(d, body, frame)
    cut_arches(d, body)
    bpy.context.view_layer.objects.active = body
    body.select_set(True)
    bpy.ops.object.shade_smooth_by_angle(angle=math.radians(35))
    body.select_set(False)
    details = join(add_details(d, body, mats, frame, stations, places), "details")
    wheels = [build_wheel("wheel_front", *d["tires"][0], mats, d["style"]),
              build_wheel("wheel_rear", *d["tires"][1], mats, d["style"])]
    calipers = [build_caliper("caliper_front", *d["tires"][0], mats),
                build_caliper("caliper_rear", *d["tires"][1], mats)]

    if args.render:
        preview(os.path.abspath(args.render), d, wheels, calipers)

    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    bpy.ops.export_scene.gltf(filepath=os.path.abspath(args.out), export_format="GLB",
                              export_yup=True, export_apply=True, export_cameras=False,
                              export_lights=False)
    if args.blend:
        os.makedirs(os.path.dirname(os.path.abspath(args.blend)), exist_ok=True)
        bpy.ops.wm.save_as_mainfile(filepath=os.path.abspath(args.blend))
    faces = sum(len(o.data.polygons) for o in bpy.data.objects if o.type == "MESH")
    print(f"QTS: wrote {args.out} ({faces} faces, details {len(details.data.polygons)})")


main()
