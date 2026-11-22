# SPDX-License-Identifier: Apache-2.0
# Checks that the map of directly read controls reaches the core as the
# front end wrote it. Axis and button numbers come out of a config file as
# ints; the bridge used to read them as floats only, so every pedal fell
# back to axis 0 and followed the steering wheel.
#   godot --headless --path godot --script res://tests/direct_map.gd
extends SceneTree

const InputReader := preload("res://scripts/input_reader.gd")

var failures: Array = []


func _check(ok: bool, what: String) -> void:
	print(("ok    " if ok else "FAIL  ") + what)
	if not ok:
		failures.append(what)


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	# Saved as the Controls screen and the wizard save it.
	var text := ""
	var axes := {"steer": 0, "throttle": 1, "brake": 5, "clutch": 6}
	for action in axes:
		text += "[direct_%s]\ndevice=\"f9260ab0333311ea8001444553540000\"\naxis=%d\nmin=-1.0\ncenter=-1.0\nmax=1.0\ninvert=%s\ncurve=%s\n\n" % [
			action, axes[action], "false" if action == "steer" else "true", "1.4" if action == "brake" else ("2" if action == "clutch" else "1")]
	text += "[direct_gear_3]\ndevice=\"f9260ab0333311ea8001444553540000\"\nbutton=15\n"
	var cfg := ConfigFile.new()
	_check(cfg.parse(text) == OK, "config parses")
	_check(typeof(cfg.get_value("direct_brake", "axis")) == TYPE_INT, "an axis number is read back as an int")
	var reader = InputReader.new()
	root.add_child(reader)
	reader.use_direct = true
	reader.shift_mode = "h_pattern"
	for section in cfg.get_sections():
		var action := section.trim_prefix("direct_")
		for key in cfg.get_section_keys(section):
			reader.direct[action][key] = cfg.get_value(section, key)
	var sim = ClassDB.instantiate("SimBridge")
	var parsed: Dictionary = sim.parse_direct_inputs(reader.direct_map())
	var got := {}
	for a in parsed.axes:
		got[a.action] = a
	for action in axes:
		_check(got.has(action) and got[action].axis == axes[action], "%s reads axis %d (got %s)" % [action, axes[action], str(got.get(action, {}).get("axis", "none"))])
	_check(got.has("throttle") and got.throttle.invert, "the throttle stays inverted")
	_check(got.has("clutch") and is_equal_approx(got.clutch.curve, 2.0), "a curve written as the int 2 is kept")
	_check(got.has("brake") and is_equal_approx(got.brake.curve, 1.4), "the brake curve is kept")
	var gear: Array = parsed.buttons.filter(func(b): return b.button == 15)
	_check(gear.size() == 1, "the third-gear gate reads button 15")
	reader.free()
	print("PASS" if failures.is_empty() else "FAILED: %d" % failures.size())
	quit(0 if failures.is_empty() else 1)
