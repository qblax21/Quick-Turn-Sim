# SPDX-License-Identifier: Apache-2.0
# Development tool: drives Ridgeline for a while and prints the input delay
# the core measures, for the per-frame path and for devices read on the
# physics thread.
#   godot --path godot --script res://tests/input_delay.gd -- [vsync|novsync] [SECONDS]
# With a controller attached, its first device is also read directly so
# both paths are timed together.
extends SceneTree


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var args := OS.get_cmdline_user_args()
	var vsync := args.size() < 1 or args[0] != "novsync"
	var seconds := float(args[1]) if args.size() > 1 else 10.0
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_ENABLED if vsync else DisplayServer.VSYNC_DISABLED)
	var main = load("res://main.tscn").instantiate()
	root.add_child(main)
	for i in range(5):
		await process_frame
	var presets: PackedStringArray = main.sim.list_ids("ffb")
	main._start_driving("hatch_tern_gt", "ridgeline_circuit", true, true, presets[0], "", false)
	# The game applies its own display settings; override them here.
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_ENABLED if vsync else DisplayServer.VSYNC_DISABLED)
	Engine.max_fps = 0
	var devices: Array = main.sim.input_scan()
	if devices.size() > 0:
		var id: String = devices[0].id
		main.sim.set_direct_inputs({"axes": [{"action": "handbrake", "device": id, "axis": 7, "min": -1.0, "center": -1.0, "max": 1.0}], "buttons": []})
	var began := Time.get_ticks_msec()
	var frame_ms := []
	var direct_ms := []
	var frame_max := 0.0
	var direct_max := 0.0
	var next_sample := began + 2000
	while Time.get_ticks_msec() - began < seconds * 1000.0:
		await process_frame
		if Time.get_ticks_msec() >= next_sample:
			next_sample += 1000
			var s: Dictionary = main.sim.status()
			frame_ms.append(s.get("input_delay_frame_ms", 0.0))
			direct_ms.append(s.get("input_delay_direct_ms", 0.0))
			frame_max = maxf(frame_max, s.get("input_delay_frame_max_ms", 0.0))
			direct_max = maxf(direct_max, s.get("input_delay_direct_max_ms", 0.0))
	var mean := func(values: Array) -> float:
		var total := 0.0
		for v in values:
			total += v
		return total / maxf(values.size(), 1)
	print("vsync %s  fps %.0f  per-frame hand-over mean %.2f ms (max %.2f)  device read to physics mean %.3f ms (max %.3f)  devices %d" % [
		"on" if vsync else "off", Engine.get_frames_per_second(), mean.call(frame_ms), frame_max,
		mean.call(direct_ms), direct_max, devices.size()])
	main.sim.stop_session()
	quit(0)
