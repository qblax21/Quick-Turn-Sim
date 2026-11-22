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
	for screen in ["welcome", "ffb"]:
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
	print("FAILURES: %d" % failures.size())
	quit(0 if failures.is_empty() else 1)
