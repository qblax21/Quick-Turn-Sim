# SPDX-License-Identifier: Apache-2.0
# Development tool: renders a screen and saves a screenshot.
#   godot --path godot --script res://tests/menu_shot.gd -- OUTPUT.png [title|select|setup|settings|pause|results]
extends SceneTree


class FixedInput:
	extends Node
	func read(_delta: float) -> Dictionary:
		return {"steer": 0.0, "throttle": 0.5, "brake": 0.0, "clutch": 0.0, "handbrake": 0.0, "buttons": 0}


func _initialize() -> void:
	_run.call_deferred()


func _wait(frames: int) -> void:
	for i in range(frames):
		await process_frame


func _run() -> void:
	var args := OS.get_cmdline_user_args()
	var mode: String = args[1] if args.size() > 1 else "title"
	var main = load("res://main.tscn").instantiate()
	root.add_child(main)
	await _wait(5)
	match mode:
		"select":
			main._show_select()
		"settings":
			main._show_settings()
		"setup":
			main._show_input_setup()
		"pause", "results":
			var presets: PackedStringArray = main.sim.list_ids("ffb")
			main._start_driving("hatch_tern_gt", "ridgeline_circuit", true, true, presets[0], "", false)
			main.reader = FixedInput.new()
			var began := Time.get_ticks_msec()
			while Time.get_ticks_msec() - began < 6000:
				await process_frame
			if mode == "pause":
				main._open_pause_menu()
			else:
				# Two laps as the core would report them.
				main.session_laps = [{"time": 104.512, "cones": 0}, {"time": 101.873, "cones": 0}]
				main._end_session()
	await _wait(8)
	# Let the title camera fly a little.
	var settle := Time.get_ticks_msec()
	while Time.get_ticks_msec() - settle < 1500:
		await process_frame
	await RenderingServer.frame_post_draw
	var error := root.get_viewport().get_texture().get_image().save_png(args[0])
	print("saved %s (%d)" % [args[0], error])
	if main.sim.is_running():
		main.sim.stop_session()
	quit(0 if error == OK else 1)
