# SPDX-License-Identifier: Apache-2.0
# Development tool: starts the real main scene, drives for a few seconds
# with fixed inputs and saves a screenshot. Needs a display (not headless).
#   godot --path godot --script res://tests/screenshot.gd -- CAR TRACK SECONDS OUTPUT.png
extends SceneTree


class FixedInput:
	extends Node
	var steer := 0.0
	var throttle := 0.5

	func read(_delta: float) -> Dictionary:
		return {"steer": steer, "throttle": throttle, "brake": 0.0, "clutch": 0.0, "handbrake": 0.0, "buttons": 0}


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var args := OS.get_cmdline_user_args()
	if args.size() < 4:
		printerr("usage: -- CAR TRACK SECONDS OUTPUT.png [STEER] [THROTTLE]")
		quit(2)
		return
	var main = load("res://main.tscn").instantiate()
	root.add_child(main)
	await process_frame
	await process_frame
	var presets: PackedStringArray = main.sim.list_ids("ffb")
	main._start_driving(args[0], args[1], true, true, presets[0], "", false)
	if not main.sim.is_running():
		printerr("session did not start: " + main.sim.last_error())
		quit(1)
		return
	var fixed := FixedInput.new()
	if args.size() > 4:
		fixed.steer = float(args[4])
	if args.size() > 5:
		fixed.throttle = float(args[5])
	main.reader = fixed
	main.hud.toggle_debug()
	if args.size() > 7:
		main.map_mode = int(args[7])
		main.hud.set_map_mode(main.map_mode)
	# Frame rate is measured with vertical sync off so it shows what the
	# scene costs; the physics timing comes from the core.
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	var began := Time.get_ticks_msec()
	var frames := 0
	var measure_from := 0
	while Time.get_ticks_msec() - began < int(float(args[2]) * 1000.0):
		await process_frame
		# Skip the first second, while the scene is being built and compiled.
		if Time.get_ticks_msec() - began > 1000:
			if measure_from == 0:
				measure_from = Time.get_ticks_msec()
			frames += 1
	var elapsed := maxf((Time.get_ticks_msec() - measure_from) / 1000.0, 0.001)
	var status: Dictionary = main.sim.status()
	print("frames %.0f per second; physics %.0f Hz, worst wake %d ticks, dropped %.3f s" % [
		frames / elapsed, status.get("tick_rate", 0.0), status.get("worst_wake_ticks", 0), status.get("dropped_time", 0.0)])
	if args.size() > 6 and args[6] == "top":
		# Freeze the scene and look straight down at the whole track, without fog.
		for child in main.get_children():
			if child is WorldEnvironment:
				child.environment.fog_enabled = false
		main.set_process(false)
		var lo := Vector3(INF, 0.0, INF)
		var hi := Vector3(-INF, 0.0, -INF)
		for path in main.sim.track_paths():
			for p in path.points:
				lo = Vector3(minf(lo.x, p.x), 0.0, minf(lo.z, p.z))
				hi = Vector3(maxf(hi.x, p.x), 0.0, maxf(hi.z, p.z))
		var span := maxf(hi.x - lo.x, hi.z - lo.z)
		main.camera.global_position = (lo + hi) * 0.5 + Vector3.UP * span * 0.95
		main.camera.look_at((lo + hi) * 0.5, Vector3.FORWARD)
		await process_frame
	elif args.size() > 6 and args[6] == "high":
		# Freeze the scene and look ahead along the road from 25 m up.
		main.set_process(false)
		main.car_view.set_seat_view(false)
		var pose: Transform3D = main.sim.car_transform()
		main.camera.global_position = pose.origin + pose.basis.z * 20.0 + Vector3.UP * 25.0
		main.camera.look_at(pose.origin - pose.basis.z * 60.0, Vector3.UP)
		await process_frame
	elif args.size() > 6 and args[6] == "side":
		# Freeze the scene and look at the car from its front-right quarter.
		main.set_process(false)
		main.car_view.set_seat_view(false)
		var xform: Transform3D = main.sim.car_transform()
		main.camera.global_position = xform.origin + xform.basis.x * 4.0 - xform.basis.z * 3.0 + Vector3.UP * 1.3
		main.camera.look_at(xform.origin, Vector3.UP)
		await process_frame
	await RenderingServer.frame_post_draw
	var image := root.get_viewport().get_texture().get_image()
	var error := image.save_png(args[3])
	print("saved %s (%d)" % [args[3], error])
	main.sim.stop_session()
	quit(0 if error == OK else 1)
