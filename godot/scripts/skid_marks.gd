# SPDX-License-Identifier: Apache-2.0
# Tire marks left by the car while a tire is sliding, from the slip the
# simulation core reports for each wheel. They stay for the session, oldest
# replaced first. Drawing only: a mark does not change grip.
extends MeshInstance3D

# Marks kept before the oldest are overwritten.
const CAPACITY := 6000
# Slip beyond which a tire leaves a mark, and the extra slip over which the
# mark reaches full darkness.
const SLIP_RATIO_START := 0.18
const SLIP_RATIO_SPAN := 0.35
const SLIP_ANGLE_START := 0.13
const SLIP_ANGLE_SPAN := 0.2
const MIN_LOAD_N := 400.0
const MIN_STEP_M := 0.12
const MAX_STEP_M := 4.0
const LIFT_M := 0.025

var radius := [0.31, 0.31, 0.31, 0.31]
var half_width := [0.11, 0.11, 0.11, 0.11]
# Per wheel: where its last mark ended, and whether it was marking.
var last_point := [Vector3.ZERO, Vector3.ZERO, Vector3.ZERO, Vector3.ZERO]
var last_side := [Vector3.ZERO, Vector3.ZERO, Vector3.ZERO, Vector3.ZERO]
var last_dark := [0.0, 0.0, 0.0, 0.0]
var marking := [false, false, false, false]
var vertices := PackedVector3Array()
var colors := PackedColorArray()
var next_quad := 0
var used_quads := 0


func setup(info: Dictionary) -> void:
	for i in range(4):
		var front := i < 2
		radius[i] = info.get("wheel_radius_front" if front else "wheel_radius_rear", 0.31)
		half_width[i] = 0.5 * info.get("tire_width_front" if front else "tire_width_rear", 0.22)
	vertices.resize(CAPACITY * 6)
	colors.resize(CAPACITY * 6)
	var material := StandardMaterial3D.new()
	material.vertex_color_use_as_albedo = true
	material.vertex_color_is_srgb = true
	material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	material.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	material.cull_mode = BaseMaterial3D.CULL_DISABLED
	material_override = material
	cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	mesh = ArrayMesh.new()


# `states` holds five numbers per wheel (see SimBridge.wheel_states); `hud`
# is the core's per-frame report with slip and load for each wheel.
func update(car_xform: Transform3D, states: PackedFloat32Array, hud: Dictionary) -> void:
	if hud.is_empty() or states.size() < 20 or not hud.has("slip_ratio"):
		return
	var changed := false
	for i in range(4):
		var o := i * 5
		var center := car_xform * Vector3(states[o], states[o + 1], states[o + 2])
		var point: Vector3 = center - car_xform.basis.y * (radius[i] - LIFT_M)
		var by_spin: float = (absf(hud.slip_ratio[i]) - SLIP_RATIO_START) / SLIP_RATIO_SPAN
		var by_slide: float = (absf(tan(hud.slip_angle[i])) - SLIP_ANGLE_START) / SLIP_ANGLE_SPAN
		var dark := clampf(maxf(by_spin, by_slide), 0.0, 1.0) * 0.55
		var sliding: bool = dark > 0.0 and hud.load[i] > MIN_LOAD_N
		if not sliding:
			marking[i] = false
			continue
		var travel: Vector3 = point - last_point[i]
		var moved := travel.length()
		if marking[i] and moved < MIN_STEP_M:
			continue
		var side: Vector3 = car_xform.basis.x * half_width[i]
		if moved > 0.001:
			side = travel.cross(car_xform.basis.y).normalized() * half_width[i]
		if marking[i] and moved < MAX_STEP_M:
			_quad(last_point[i], last_side[i], last_dark[i], point, side, dark)
			changed = true
		last_point[i] = point
		last_side[i] = side
		last_dark[i] = dark
		marking[i] = true
	if changed:
		_rebuild()


func _quad(a: Vector3, a_side: Vector3, a_dark: float, b: Vector3, b_side: Vector3, b_dark: float) -> void:
	var k := next_quad * 6
	var from := Color(0.02, 0.02, 0.02, a_dark)
	var to := Color(0.02, 0.02, 0.02, b_dark)
	var corners := [a - a_side, b - b_side, a + a_side, a + a_side, b - b_side, b + b_side]
	var shades := [from, to, from, from, to, to]
	for j in range(6):
		vertices[k + j] = corners[j]
		colors[k + j] = shades[j]
	next_quad = (next_quad + 1) % CAPACITY
	used_quads = mini(used_quads + 1, CAPACITY)


func _rebuild() -> void:
	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	# Unused quads are all at the origin with no area, so the whole buffer
	# can be handed over once it has wrapped.
	arrays[Mesh.ARRAY_VERTEX] = vertices if used_quads == CAPACITY else vertices.slice(0, used_quads * 6)
	arrays[Mesh.ARRAY_COLOR] = colors if used_quads == CAPACITY else colors.slice(0, used_quads * 6)
	var array_mesh: ArrayMesh = mesh
	array_mesh.clear_surfaces()
	array_mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
