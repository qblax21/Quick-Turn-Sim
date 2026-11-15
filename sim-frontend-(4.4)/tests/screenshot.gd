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
	var began := Time.get_ticks_msec()
	while Time.get_ticks_msec() - began < int(float(args[2]) * 1000.0):
		await process_frame
	if args.size() > 6 and args[6] == "side":
		# Freeze the scene and look at the car from its front-right quarter.
		main.set_process(false)
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
