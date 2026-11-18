# SPDX-License-Identifier: Apache-2.0
# Development tool: drives a session with fixed inputs, leaves it (which
# saves its replay), opens that replay, waits, seeks and saves a screenshot
# from each camera. Needs a display.
#   godot --path godot --script res://tests/replay_shot.gd -- CAR TRACK SECONDS OUTPUT_PREFIX
extends SceneTree


class FixedInput:
	extends Node
	func read(_delta: float) -> Dictionary:
		return {"steer": 0.0, "throttle": 0.6, "brake": 0.0, "clutch": 0.0, "handbrake": 0.0, "buttons": 0}


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var args := OS.get_cmdline_user_args()
	var main = load("res://main.tscn").instantiate()
	root.add_child(main)
	await process_frame
	await process_frame
	var presets: PackedStringArray = main.sim.list_ids("ffb")
	main._start_driving(args[0], args[1], true, true, presets[0], "", false)
	main.reader = FixedInput.new()
	var began := Time.get_ticks_msec()
	while Time.get_ticks_msec() - began < int(float(args[2]) * 1000.0):
		await process_frame
	main._show_menu()
	await process_frame
	var dir := ProjectSettings.globalize_path("user://replays")
	var names := Array(DirAccess.get_files_at(dir))
	names.sort()
	if names.is_empty():
		printerr("no replay was saved")
		quit(1)
		return
	var path: String = dir.path_join(names[names.size() - 1])
	print("saved replay ", path)
	main._show_replays()
	await process_frame
	main._watch_replay(path)
	var view = main.replay_view
	# Let it prepare, then look at the middle of the drive from each camera.
	began = Time.get_ticks_msec()
	while not view.prepared and Time.get_ticks_msec() - began < 30000:
		await process_frame
	print("prepared in %d ms" % (Time.get_ticks_msec() - began))
	var info: Dictionary = main.sim.replay_info()
	print("duration %.2f s, mismatch tick %d" % [info.duration, info.mismatch_tick])
	for c in range(4):
		view.camera_index = c
		view.time = float(args[2]) * 0.7
		view.playing = false
		for i in range(6):
			await process_frame
		await RenderingServer.frame_post_draw
		var file: String = "%s_%d.png" % [args[3], c]
		root.get_viewport().get_texture().get_image().save_png(file)
		print("saved ", file)
	quit(0 if info.mismatch_tick < 0 else 1)
