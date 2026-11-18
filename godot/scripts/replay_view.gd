# SPDX-License-Identifier: Apache-2.0
# Watching a replay. The core re-simulates the recording once (a slice each
# frame, so the window stays responsive) and keeps the car's pose; this
# script only asks where the car was at a moment and draws it. Cameras:
# trackside, chase, orbit and driver's seat.
#
#   Space        play or pause
#   Left, Right  back or forward 5 s (with Shift: 0.5 s)
#   Up, Down     play faster or slower
#   C            next camera
#   H            hide or show everything on screen except the scene
#   Home         back to the start
#   Esc          leave
extends Node

const World := preload("res://scripts/world.gd")
const CarView := preload("res://scripts/car_view.gd")

signal closed

const CAMERAS := ["Trackside", "Chase", "Orbit", "Driver's seat"]
const SPEEDS := [0.125, 0.25, 0.5, 1.0, 2.0, 4.0]
# Time spent preparing the replay each frame, ms.
const PREPARE_BUDGET_MS := 10
# Trackside posts: one every this many metres of path, this far to the side
# and this high.
const POST_SPACING_M := 70.0
const POST_OFFSET_M := 17.0
const POST_HEIGHT_M := 3.5

var sim
var camera: Camera3D
var world
var car_view
var time := 0.0
var playing := true
var speed_index := 3
var camera_index := 0
var duration := 0.0
var prepared := false
var posts := PackedVector3Array()
var post_index := -1
var orbit_angle := 0.0
var orbit_drag := false

var layer: CanvasLayer
var info_label: Label
var help_label: Label
var notice_label: Label
var slider: HSlider
var slider_busy := false


func setup(bridge, scene_camera: Camera3D) -> void:
	sim = bridge
	camera = scene_camera


func _ready() -> void:
	world = World.new()
	add_child(world)
	world.build(sim)
	car_view = CarView.new()
	add_child(car_view)
	car_view.build(sim.car_info())
	duration = sim.replay_info().get("duration", 0.0)
	_place_posts()
	_build_overlay()


# Camera posts beside the first path, alternating sides.
func _place_posts() -> void:
	var paths: Array = sim.track_paths()
	if paths.is_empty():
		return
	var pts: PackedVector3Array = paths[0].points
	var distances: PackedFloat32Array = paths[0].distances
	var next := 0.0
	var side := 1.0
	var spots := PackedVector3Array()
	for i in range(1, pts.size()):
		if distances[i] < next:
			continue
		var ahead: Vector3 = pts[i] - pts[i - 1]
		ahead.y = 0.0
		if ahead.length() < 0.01:
			continue
		ahead = ahead.normalized()
		var left := Vector3(ahead.z, 0.0, -ahead.x)
		spots.append(pts[i] + left * side * POST_OFFSET_M)
		side = -side
		next = distances[i] + POST_SPACING_M
	posts = sim.ground_heights(spots)
	for k in range(posts.size()):
		posts[k] += Vector3.UP * POST_HEIGHT_M


func _build_overlay() -> void:
	layer = CanvasLayer.new()
	add_child(layer)
	info_label = Label.new()
	info_label.add_theme_font_size_override("font_size", 22)
	info_label.set_anchors_and_offsets_preset(Control.PRESET_TOP_LEFT, Control.PRESET_MODE_MINSIZE, 24)
	layer.add_child(info_label)
	notice_label = Label.new()
	notice_label.add_theme_font_size_override("font_size", 16)
	notice_label.add_theme_color_override("font_color", Color(1.0, 0.85, 0.4))
	notice_label.set_anchors_and_offsets_preset(Control.PRESET_CENTER_TOP, Control.PRESET_MODE_MINSIZE, 12)
	notice_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	notice_label.grow_horizontal = Control.GROW_DIRECTION_BOTH
	layer.add_child(notice_label)
	help_label = Label.new()
	help_label.add_theme_font_size_override("font_size", 14)
	help_label.text = "Space play/pause   Left/Right -5/+5 s (Shift: 0.5 s)   Up/Down speed   C camera   drag: turn orbit camera   H hide all   Home start   Esc leave"
	help_label.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_WIDE, Control.PRESET_MODE_MINSIZE, 24)
	help_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	help_label.grow_vertical = Control.GROW_DIRECTION_BEGIN
	help_label.position.y -= 34
	layer.add_child(help_label)
	slider = HSlider.new()
	slider.min_value = 0.0
	slider.max_value = maxf(duration, 0.01)
	slider.step = 0.01
	slider.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_WIDE, Control.PRESET_MODE_MINSIZE, 24)
	slider.grow_vertical = Control.GROW_DIRECTION_BEGIN
	slider.offset_left = 24
	slider.offset_right = -24
	slider.drag_started.connect(func() -> void: slider_busy = true)
	slider.drag_ended.connect(func(_changed: bool) -> void: slider_busy = false)
	slider.value_changed.connect(func(v: float) -> void:
		if slider_busy:
			time = v)
	layer.add_child(slider)


func _process(delta: float) -> void:
	if not prepared:
		var began := Time.get_ticks_msec()
		var progress := 0.0
		while Time.get_ticks_msec() - began < PREPARE_BUDGET_MS:
			progress = sim.replay_prepare(500)
			if progress >= 1.0:
				prepared = true
				break
	var info: Dictionary = sim.replay_info()
	var available: float = info.get("prepared", 0.0) if not prepared else duration
	if playing and not slider_busy:
		time += delta * SPEEDS[speed_index]
		if time >= duration and prepared:
			time = duration
			playing = false
	time = clampf(time, 0.0, available)

	var frame: Dictionary = sim.replay_frame(time)
	if frame.is_empty():
		return
	var xform: Transform3D = frame.transform
	car_view.update(xform, frame.wheels, frame.steering_wheel_angle)
	world.update_cones(sim.replay_cones(time))
	_update_camera(xform, delta)
	_update_overlay(frame, info, available)


func _update_camera(xform: Transform3D, delta: float) -> void:
	var target := xform.origin + Vector3.UP * 0.6
	car_view.set_seat_view(camera_index == 3)
	match camera_index:
		0:
			_trackside(target)
		1:
			var back := xform.basis.z
			back.y = 0.0
			back = back.normalized() if back.length() > 0.01 else Vector3.BACK
			var goal := xform.origin + back * 7.5 + Vector3.UP * 2.8
			camera.global_position = camera.global_position.lerp(goal, 1.0 - exp(-6.0 * delta))
			camera.look_at(target, Vector3.UP)
			camera.fov = 65.0
		2:
			if not orbit_drag:
				orbit_angle += delta * 0.35
			var offset := Vector3(sin(orbit_angle), 0.0, cos(orbit_angle)) * 7.0
			camera.global_position = xform.origin + offset + Vector3.UP * 2.2
			camera.look_at(target, Vector3.UP)
			camera.fov = 60.0
		3:
			camera.global_transform = xform * Transform3D(Basis.IDENTITY, car_view.seat_offset)
			camera.fov = 65.0


# The post nearest the car films it, zoomed so the car stays about the same
# size; it changes post only when another is clearly nearer.
func _trackside(target: Vector3) -> void:
	if posts.is_empty():
		return
	var nearest := 0
	var best := INF
	for k in range(posts.size()):
		var d := posts[k].distance_squared_to(target)
		if d < best:
			best = d
			nearest = k
	if post_index < 0 or post_index >= posts.size() or best < 0.6 * posts[post_index].distance_squared_to(target):
		post_index = nearest
	var spot := posts[post_index]
	camera.global_position = spot
	camera.look_at(target, Vector3.UP)
	var distance := spot.distance_to(target)
	camera.fov = clampf(rad_to_deg(2.0 * atan(5.0 / maxf(distance, 0.1))), 6.0, 60.0)


func _format_time(seconds: float) -> String:
	var minutes := int(seconds / 60.0)
	return "%d:%06.3f" % [minutes, seconds - minutes * 60.0]


func _update_overlay(frame: Dictionary, info: Dictionary, available: float) -> void:
	var lines := [
		"REPLAY   %s / %s   x%s%s" % [_format_time(time), _format_time(duration), str(SPEEDS[speed_index]), "" if playing else "   paused"],
		"%s camera" % CAMERAS[camera_index],
		"%3.0f km/h   gear %d   %.0f rpm" % [frame.speed * 3.6, frame.gear, frame.engine_rpm],
		"throttle %3.0f%%   brake %3.0f%%   steer %+4.0f%%" % [frame.throttle * 100.0, frame.brake * 100.0, frame.steer * 100.0],
	]
	if frame.run_time >= 0.0:
		lines.append("run %s" % _format_time(frame.run_time))
	info_label.text = "\n".join(lines)
	if not slider_busy:
		slider.set_value_no_signal(time)
	var notices := []
	if not prepared:
		notices.append("preparing the replay: %.0f s of %.0f s ready" % [available, duration])
	if info.get("mismatch_tick", -1) >= 0:
		notices.append("this replay no longer reproduces exactly from %.1f s (recorded by build %s): what is shown after that is not what was driven" % [
			info.mismatch_tick / maxf(info.get("tick_rate", 1000.0), 1.0), info.get("recorded_by", "?")])
	notice_label.text = "\n".join(notices)


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton and event.button_index == MOUSE_BUTTON_LEFT:
		orbit_drag = event.pressed and camera_index == 2
	elif event is InputEventMouseMotion and orbit_drag:
		orbit_angle -= event.relative.x * 0.01
	if not (event is InputEventKey) or not event.pressed:
		return
	var step := 0.5 if event.shift_pressed else 5.0
	match event.physical_keycode:
		KEY_SPACE:
			if time >= duration:
				time = 0.0
			playing = not playing
		KEY_LEFT:
			time = maxf(time - step, 0.0)
		KEY_RIGHT:
			time += step
		KEY_UP:
			speed_index = mini(speed_index + 1, SPEEDS.size() - 1)
		KEY_DOWN:
			speed_index = maxi(speed_index - 1, 0)
		KEY_C:
			camera_index = (camera_index + 1) % CAMERAS.size()
			post_index = -1
		KEY_H:
			layer.visible = not layer.visible
		KEY_HOME:
			time = 0.0
		KEY_ESCAPE:
			closed.emit()
	get_viewport().set_input_as_handled()


func close() -> void:
	sim.replay_close()
