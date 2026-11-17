# SPDX-License-Identifier: Apache-2.0
# A small map of the track with the car and the ghost on it. North-up:
# the world's -Z is at the top. Display only; every position comes from the
# simulation core.
extends Control

const SIZE := 220.0
const MARGIN := 14.0
const TRACK_COLOR := Color(0.95, 0.95, 0.95, 0.9)
const CONE_COLOR := Color(1.0, 0.55, 0.1, 0.9)
const START_COLOR := Color(0.25, 0.95, 0.35)
const FINISH_COLOR := Color(0.95, 0.95, 0.95)
const CAR_COLOR := Color(1.0, 0.85, 0.15)
const GHOST_COLOR := Color(0.6, 0.85, 1.0, 0.9)

var lines: Array = []
var cones := PackedVector2Array()
var gates: Array = []
var origin := Vector2.ZERO
var scale_px := 1.0
var usable := false
var car_point := Vector2.ZERO
var car_forward := Vector2.UP
var ghost_point := Vector2.ZERO
var ghost_shown := false


func _flat(p: Vector3) -> Vector2:
	return Vector2(p.x, p.z)


func setup(sim) -> void:
	custom_minimum_size = Vector2(SIZE, SIZE)
	size = Vector2(SIZE, SIZE)
	mouse_filter = Control.MOUSE_FILTER_IGNORE
	var lo := Vector2(INF, INF)
	var hi := Vector2(-INF, -INF)
	var all: Array = []
	for path in sim.track_paths():
		var line := PackedVector2Array()
		var points: PackedVector3Array = path.points
		for p in points:
			line.append(_flat(p))
		if line.size() >= 2:
			lines.append(line)
			all.append(line)
	for marker in sim.track_markers():
		if marker.kind == "circle":
			var ring := PackedVector2Array()
			for i in range(49):
				var a := TAU * i / 48.0
				ring.append(_flat(marker.center) + Vector2(cos(a), sin(a)) * marker.radius)
			lines.append(ring)
			all.append(ring)
	for p in sim.track_cones():
		cones.append(_flat(p))
	all.append(cones)
	for gate in sim.track_gates():
		var ends := PackedVector2Array([_flat(gate.a), _flat(gate.b)])
		gates.append([ends, gate.kind == "start"])
		all.append(ends)
	for group in all:
		for p in group:
			lo = Vector2(minf(lo.x, p.x), minf(lo.y, p.y))
			hi = Vector2(maxf(hi.x, p.x), maxf(hi.y, p.y))
	usable = lo.x < hi.x or lo.y < hi.y
	if not usable:
		visible = false
		return
	var span := maxf(maxf(hi.x - lo.x, hi.y - lo.y), 1.0)
	scale_px = (SIZE - 2.0 * MARGIN) / span
	# Centre the track in the square.
	origin = (lo + hi) * 0.5


func _to_map(p: Vector2) -> Vector2:
	return (p - origin) * scale_px + Vector2(SIZE, SIZE) * 0.5


func set_cars(car_xform: Transform3D, ghost_visible: bool, ghost_xform: Transform3D) -> void:
	car_point = _flat(car_xform.origin)
	var forward := -car_xform.basis.z
	var flat := Vector2(forward.x, forward.z)
	if flat.length() > 0.001:
		car_forward = flat.normalized()
	ghost_shown = ghost_visible
	if ghost_visible:
		ghost_point = _flat(ghost_xform.origin)
	queue_redraw()


func _draw() -> void:
	if not usable:
		return
	draw_rect(Rect2(Vector2.ZERO, Vector2(SIZE, SIZE)), Color(0.0, 0.0, 0.0, 0.38))
	for line in lines:
		var mapped := PackedVector2Array()
		for p in line:
			mapped.append(_to_map(p))
		draw_polyline(mapped, TRACK_COLOR, 2.5, true)
	for p in cones:
		draw_circle(_to_map(p), 1.3, CONE_COLOR)
	for gate in gates:
		var ends: PackedVector2Array = gate[0]
		var middle := _to_map((ends[0] + ends[1]) * 0.5)
		var across := (ends[1] - ends[0]).normalized() * 6.0
		draw_line(middle - across, middle + across, START_COLOR if gate[1] else FINISH_COLOR, 3.0)
	if ghost_shown:
		draw_circle(_to_map(ghost_point), 4.0, GHOST_COLOR)
	# The car: an arrow pointing the way it faces.
	var at := _to_map(car_point)
	var side := Vector2(-car_forward.y, car_forward.x)
	draw_colored_polygon(PackedVector2Array([
		at + car_forward * 9.0,
		at - car_forward * 6.0 + side * 6.0,
		at - car_forward * 6.0 - side * 6.0,
	]), CAR_COLOR)
