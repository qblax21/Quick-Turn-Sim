# SPDX-License-Identifier: Apache-2.0
# Checks the setup wizard's arithmetic and device profiles (D-085), and
# renders its screens. Needs a display for the screenshots.
#   godot --path godot --script res://tests/wizard_check.gd -- OUTPUT_PREFIX
extends SceneTree

const Wizard := preload("res://scripts/wizard.gd")
const InputReader := preload("res://scripts/input_reader.gd")

var failures: Array = []


func _check(ok: bool, what: String) -> void:
	print(("ok    " if ok else "FAIL  ") + what)
	if not ok:
		failures.append(what)


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var prefix: String = OS.get_cmdline_user_args()[0]
	# The test drive saves controls and settings as the wizard does: the
	# player's own files are put back at the end.
	var kept := {}
	for f in ["user://input.cfg", "user://settings.cfg"]:
		kept[f] = FileAccess.get_file_as_bytes(f) if FileAccess.file_exists(f) else null
	# A quarter turn reads 0.2 of the half range on a 900-degree base.
	_check(is_equal_approx(Wizard.rotation_from_quarter_turn(0.0, 0.2), 900.0), "a quarter turn of 0.2 means 900 degrees")
	_check(absf(Wizard.rotation_from_quarter_turn(0.05, -0.11667) - 1080.0) < 0.5, "measured from an off-zero centre, either way")
	_check(Wizard.rotation_from_quarter_turn(0.0, 0.0) == 0.0, "no movement, no rotation")
	_check(is_equal_approx(Wizard.gain_for(6.0), 0.75 * 6.0 / 17.0), "gain for 6 N.m puts 17 N.m at the knee")
	_check(Wizard.gain_for(25.0) == 1.0, "a strong base runs the real torque")
	# Pedal shaping matches the core's (same numbers as its unit test).
	var m := {"deadzone_low": 0.05, "deadzone_high": 0.05, "curve": 2.0}
	_check(InputReader.shape_pedal(0.04, m) == 0.0 and InputReader.shape_pedal(0.96, m) == 1.0, "deadzones at both ends")
	_check(is_equal_approx(InputReader.shape_pedal(0.5, m), 0.25), "the curve squares mid travel")
	# A profile saved for a device comes back.
	var reader = InputReader.new()
	root.add_child(reader)
	var name := "Wizard Test Wheel %d" % OS.get_process_id()
	reader.direct["brake"]["curve"] = 1.7
	reader.direct["brake"]["deadzone_low"] = 0.03
	var path: String = reader.save_profile(name, {"rotation_deg": 1080.0, "wheel_model": "fanatec_csl_elite"})
	reader.direct["brake"]["curve"] = 1.0
	var extra: Dictionary = reader.load_profile(name)
	_check(extra.get("found", false) and is_equal_approx(reader.direct["brake"]["curve"], 1.7) and is_equal_approx(reader.direct["brake"]["deadzone_low"], 0.03), "the profile restores the pedal shaping")
	_check(extra.get("rotation_deg", 0.0) == 1080.0 and extra.get("wheel_model", "") == "fanatec_csl_elite", "and the wheel's rotation and model")
	_check(reader.load_profile("no such wheel %d" % OS.get_process_id()).is_empty(), "an unknown wheel has no profile")
	DirAccess.remove_absolute(ProjectSettings.globalize_path(path))
	# And the folder, if this made it and it is empty.
	DirAccess.remove_absolute(ProjectSettings.globalize_path("user://input_profiles"))
	reader.queue_free()

	# The screens, as a player meets them.
	var main = load("res://main.tscn").instantiate()
	root.add_child(main)
	for i in 5:
		await process_frame
	main._show_wizard()
	await process_frame
	var wizard = null
	for c in main.get_children():
		if c.get_script() == Wizard:
			wizard = c
	_check(wizard != null, "the wizard opens")

	# D-131: the base found by the name it reports, and its preset picked.
	wizard.device_name = "FANATEC CSL Elite Wheel Base"
	wizard._recognise_base()
	_check(wizard.auto_model == "fanatec_csl_elite" and wizard.models[wizard.model_index].peak_torque_nm == 6.0,
		"the CSL Elite is recognised by its device name, 6 N.m")
	_check(main.sim.ffb_config().get("id", "") == "fanatec_csl_elite", "and its preset is loaded")
	wizard.device_name = "Some Other Wheel"
	wizard.model_index = -1
	wizard._recognise_base()
	_check(wizard.auto_model == "" and wizard.model_index == -1, "an unknown base is left to the player")

	# The rest check: a pedal reading 3 % at rest gets a 4 % deadzone.
	wizard.pedal_axis = {"throttle": 0}
	main.reader.direct["throttle"] = {"device": "none", "axis": 0, "min": 0.0, "max": 1.0, "invert": false, "deadzone_low": 0.0, "deadzone_high": 0.0, "curve": 1.0}
	wizard.device = ""
	wizard._go(Wizard.STEPS.find("check"))
	wizard.check_pedal_peak = {"throttle": 0.03}
	wizard.check_started = Time.get_ticks_msec() - 3000
	wizard.set_process(false)
	# _raw reads 0 for an unknown device, so the peak stays 3 %.
	wizard._watch_check()
	_check(is_equal_approx(main.reader.direct["throttle"]["deadzone_low"], 0.04), "a pedal at 3 % at rest gets a 4 % deadzone")
	wizard.pedal_axis = {}
	wizard.check_report = ""

	# The test drive, end to end: 20 s on Ridgeline with the computer
	# driving (no one is at a wheel here), then the gain from its clipping.
	for i in wizard.models.size():
		if wizard.models[i].id == "fanatec_csl_elite":
			wizard.model_index = i
	wizard.apply_for_drive()
	var gain_before: float = main.sim.ffb_config().get("gain", 0.0)
	main.self_testing = true
	main._wizard_test_drive("", wizard)
	_check(main.sim.is_running() and not wizard.visible, "the test drive starts with the wizard hidden")
	main.sim.set_ai_driver("grip", 1.0)
	while not main.test_drive.is_empty():
		await process_frame
	_check(wizard.visible and main.sim.is_running() == false, "and comes back to the wizard")
	print("test drive: ", wizard.test_report)
	_check(wizard.tested_gain > 0.0 and wizard.tested_gain <= Wizard.gain_for(6.0) + 1e-6, "a gain was set from the drive, never above the hard-cornering estimate (%.2f, was %.2f)" % [wizard.tested_gain, gain_before])
	# A drive that clipped too much lowers it.
	wizard.test_drive_result({"clip_percent": 6.0, "suggested_gain": 0.18, "gain": 0.26})
	_check(is_equal_approx(wizard.tested_gain, 0.18), "a drive clipping 6 % lowers the gain to the helper's 0.18")
	wizard.tested_gain = -1.0
	wizard.test_drive_result({"clip_percent": 0.0, "suggested_gain": 0.9, "gain": 0.26})
	# Again at the new gain: clipping under 2 %.
	wizard.apply_for_drive()
	main._wizard_test_drive("", wizard)
	main.sim.set_ai_driver("grip", 1.0)
	var clip_at_new := -1.0
	while not main.test_drive.is_empty():
		if main.sim.is_running():
			clip_at_new = main.sim.status().get("clip_lap_percent", -1.0)
		await process_frame
	print("test drive again: ", wizard.test_report)
	_check(clip_at_new >= 0.0 and clip_at_new < wizard.CLIP_LIMIT_PERCENT, "at that gain the same drive clips %.2f %% of the time, under 2 %%" % clip_at_new)

	for screen in ["welcome", "check", "ffb", "test_drive"]:
		wizard._go(Wizard.STEPS.find(screen))
		if screen == "ffb":
			wizard.model_index = 0
			for i in wizard.models.size():
				if wizard.models[i].id == "fanatec_csl_elite":
					wizard.model_index = i
			wizard._go(Wizard.STEPS.find("ffb"))
		for i in 10:
			await process_frame
		await RenderingServer.frame_post_draw
		root.get_viewport().get_texture().get_image().save_png("%s_%s.png" % [prefix, screen])
	for f in kept:
		if kept[f] == null:
			DirAccess.remove_absolute(ProjectSettings.globalize_path(f))
		else:
			var out := FileAccess.open(f, FileAccess.WRITE)
			out.store_buffer(kept[f])
			out.close()
	print("FAILURES: %d" % failures.size())
	quit(0 if failures.is_empty() else 1)
