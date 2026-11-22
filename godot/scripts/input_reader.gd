# SPDX-License-Identifier: Apache-2.0
# Reads keyboard, gamepad and wheel input and turns it into the values the
# simulation expects. Mapping and calibration are stored in user://input.cfg.
# This is input handling only; nothing here affects how the car behaves
# beyond the values it sends.
extends Node

const CONFIG_PATH := "user://input.cfg"
const AXIS_ACTIONS := ["steer", "throttle", "brake", "clutch", "handbrake"]
const BUTTON_ACTIONS := ["shift_up", "shift_down", "reset", "handbrake_button", "starter",
	"gear_1", "gear_2", "gear_3", "gear_4", "gear_5", "gear_6", "gear_r"]
const GEAR_GATES := {"gear_1": 1, "gear_2": 2, "gear_3": 3, "gear_4": 4, "gear_5": 5, "gear_6": 6, "gear_r": -1}
# Gear request meaning "no lever position": sequential shifting.
const NO_GEAR := -128

# Bit values defined by the simulation core (qts_core::input::buttons).
const BUTTON_SHIFT_UP := 1
const BUTTON_SHIFT_DOWN := 2
const BUTTON_RESET := 4
const BUTTON_STARTER := 8

# Each axis mapping: device (-1 = unmapped), axis index, calibrated minimum,
# centre and maximum of the raw value, and whether to flip it.
var axes := {}
# Each button mapping: device (-1 = unmapped) and button index.
var buttons := {}
# The same controls read straight from the device on the physics thread:
# device id ("" = not mapped), axis or button index, and calibration in the
# device's raw units (-1..1). These take over from the mappings above while
# driving when `use_direct` is on; the mappings above are still read so the
# core can measure how much later they arrive.
var direct := {}
var use_direct := true
# "sequential" (up and down) or "h_pattern" (a shifter with a gate per gear).
var shift_mode := "sequential"
var keyboard_steer := 0.0
# The core, for matching the wheel's rotation to the car; set by main.
var sim_bridge = null
var keyboard_throttle := 0.0
var keyboard_brake := 0.0


func _ready() -> void:
	for action in AXIS_ACTIONS:
		axes[action] = {"device": -1, "axis": 0, "min": -1.0, "center": 0.0, "max": 1.0, "invert": false,
			"deadzone_low": 0.0, "deadzone_high": 0.0, "curve": 1.0}
	for action in BUTTON_ACTIONS:
		buttons[action] = {"device": -1, "button": 0}
	for action in AXIS_ACTIONS:
		direct[action] = {"device": "", "axis": 0, "min": -1.0, "center": 0.0, "max": 1.0, "invert": false,
			"deadzone_low": 0.0, "deadzone_high": 0.0, "curve": 1.0}
	for action in BUTTON_ACTIONS:
		direct[action] = {"device": "", "button": 0}
	load_config()


func load_config() -> void:
	var cfg := ConfigFile.new()
	if cfg.load(CONFIG_PATH) != OK:
		return
	for action in AXIS_ACTIONS:
		for key in axes[action].keys():
			axes[action][key] = cfg.get_value("axis_" + action, key, axes[action][key])
	for action in BUTTON_ACTIONS:
		for key in buttons[action].keys():
			buttons[action][key] = cfg.get_value("button_" + action, key, buttons[action][key])
	for action in direct.keys():
		for key in direct[action].keys():
			direct[action][key] = cfg.get_value("direct_" + action, key, direct[action][key])
	use_direct = cfg.get_value("input", "use_direct", true)
	shift_mode = cfg.get_value("input", "shift_mode", "sequential")


func save_config() -> void:
	var cfg := ConfigFile.new()
	for action in AXIS_ACTIONS:
		for key in axes[action].keys():
			cfg.set_value("axis_" + action, key, axes[action][key])
	for action in BUTTON_ACTIONS:
		for key in buttons[action].keys():
			cfg.set_value("button_" + action, key, buttons[action][key])
	for action in direct.keys():
		for key in direct[action].keys():
			cfg.set_value("direct_" + action, key, direct[action][key])
	cfg.set_value("input", "use_direct", use_direct)
	cfg.set_value("input", "shift_mode", shift_mode)
	cfg.save(CONFIG_PATH)


func has_direct(action: String) -> bool:
	return direct.has(action) and direct[action].device != ""


# The controls to read on the physics thread, in the form the core takes.
func direct_map() -> Dictionary:
	var out := {"axes": [], "buttons": []}
	if not use_direct:
		return out
	for action in direct.keys():
		var m: Dictionary = direct[action]
		if m.device == "":
			continue
		# Shifter gates only in H-pattern mode.
		if GEAR_GATES.has(action) and shift_mode != "h_pattern":
			continue
		var entry := m.duplicate()
		entry["action"] = action
		if m.has("axis"):
			out.axes.append(entry)
		else:
			out.buttons.append(entry)
	return out


# Calibrated value of a directly read axis from a raw device value, the
# same arithmetic as the core uses.
func direct_calibrated(action: String, raw: float) -> float:
	var m: Dictionary = direct[action]
	var value := 0.0
	if action == "steer":
		if raw >= m.center:
			value = (raw - m.center) / maxf(m.max - m.center, 0.001)
		else:
			value = (raw - m.center) / maxf(m.center - m.min, 0.001)
		value = clampf(value, -1.0, 1.0)
		return -value if m.invert else value
	value = clampf((raw - m.min) / maxf(m.max - m.min, 0.001), 0.0, 1.0)
	return shape_pedal(1.0 - value if m.invert else value, m)


# Pedal deadzones and curve, the same arithmetic as the core's PedalShape
# (D-085): travel inside the low deadzone reads zero, inside the high one
# full, and the rest is raised to the curve's power.
static func shape_pedal(travel: float, m: Dictionary) -> float:
	var lo := clampf(m.get("deadzone_low", 0.0), 0.0, 0.45)
	var hi := clampf(m.get("deadzone_high", 0.0), 0.0, 0.45)
	var x := clampf((travel - lo) / maxf(1.0 - lo - hi, 0.05), 0.0, 1.0)
	var curve := clampf(m.get("curve", 1.0), 0.2, 5.0)
	if curve == 1.0 or x == 0.0:
		return x
	return pow(x, curve)


# A file-name-safe key for a device's profile.
static func profile_key(device_name: String) -> String:
	var key := ""
	for c in device_name.to_lower():
		key += c if (c >= "a" and c <= "z") or (c >= "0" and c <= "9") else "_"
	return key.strip_edges().left(60)


# Saves the current mappings as the profile of a device (D-085), with any
# extra values (force feedback) the caller adds.
func save_profile(device_name: String, extra: Dictionary) -> String:
	DirAccess.make_dir_recursive_absolute(ProjectSettings.globalize_path("user://input_profiles"))
	var path := "user://input_profiles/%s.cfg" % profile_key(device_name)
	var cfg := ConfigFile.new()
	cfg.set_value("profile", "device_name", device_name)
	for action in direct.keys():
		for key in direct[action].keys():
			cfg.set_value("direct_" + action, key, direct[action][key])
	for key in extra.keys():
		cfg.set_value("extra", key, extra[key])
	cfg.save(path)
	return path


# Loads a device's profile into the mappings; returns its extra values, or
# an empty dictionary if there is none.
func load_profile(device_name: String) -> Dictionary:
	var cfg := ConfigFile.new()
	if cfg.load("user://input_profiles/%s.cfg" % profile_key(device_name)) != OK:
		return {}
	for action in direct.keys():
		for key in direct[action].keys():
			direct[action][key] = cfg.get_value("direct_" + action, key, direct[action][key])
	use_direct = true
	var extra := {"found": true}
	if cfg.has_section("extra"):
		for key in cfg.get_section_keys("extra"):
			extra[key] = cfg.get_value("extra", key)
	return extra


func is_mapped(action: String) -> bool:
	return axes[action].device >= 0 and axes[action].device in Input.get_connected_joypads()


# Raw value of a mapped axis, or 0 if unmapped.
func raw_axis(action: String) -> float:
	var m: Dictionary = axes[action]
	if m.device < 0:
		return 0.0
	return Input.get_joy_axis(m.device, m.axis)


# Calibrated steering in -1..1, positive to the RIGHT as devices report it.
func _calibrated_centered(action: String) -> float:
	var m: Dictionary = axes[action]
	var raw := raw_axis(action)
	var value := 0.0
	if raw >= m.center:
		value = (raw - m.center) / maxf(m.max - m.center, 0.001)
	else:
		value = (raw - m.center) / maxf(m.center - m.min, 0.001)
	value = clampf(value, -1.0, 1.0)
	return -value if m.invert else value


# Calibrated pedal in 0..1.
func _calibrated_pedal(action: String) -> float:
	var m: Dictionary = axes[action]
	var value := clampf((raw_axis(action) - m.min) / maxf(m.max - m.min, 0.001), 0.0, 1.0)
	return shape_pedal(1.0 - value if m.invert else value, m)


func _button(action: String) -> bool:
	var m: Dictionary = buttons[action]
	return m.device >= 0 and Input.is_joy_button_pressed(m.device, m.button)


func _first_gamepad() -> int:
	var pads := Input.get_connected_joypads()
	return pads[0] if pads.size() > 0 else -1


# Returns steer (-1 right .. 1 left), pedals (0..1) and the button bit field.
func read(delta: float) -> Dictionary:
	var pad := _first_gamepad()

	# Keyboard, eased so a key press is not a step input.
	var key_left := Input.is_physical_key_pressed(KEY_A) or Input.is_physical_key_pressed(KEY_LEFT)
	var key_right := Input.is_physical_key_pressed(KEY_D) or Input.is_physical_key_pressed(KEY_RIGHT)
	var key_gas := Input.is_physical_key_pressed(KEY_W) or Input.is_physical_key_pressed(KEY_UP)
	var key_brake := Input.is_physical_key_pressed(KEY_S) or Input.is_physical_key_pressed(KEY_DOWN)
	var steer_target := (1.0 if key_left else 0.0) - (1.0 if key_right else 0.0)
	var steer_rate := 1.6 if steer_target != 0.0 else 3.0
	keyboard_steer = move_toward(keyboard_steer, steer_target * 0.6, steer_rate * delta)
	keyboard_throttle = move_toward(keyboard_throttle, 1.0 if key_gas else 0.0, 4.0 * delta)
	keyboard_brake = move_toward(keyboard_brake, 1.0 if key_brake else 0.0, 6.0 * delta)

	var steer := keyboard_steer
	var throttle := keyboard_throttle
	var brake := keyboard_brake
	var clutch := 1.0 if Input.is_physical_key_pressed(KEY_SHIFT) else 0.0
	var handbrake := 1.0 if Input.is_physical_key_pressed(KEY_SPACE) else 0.0

	# Mapped wheel or controller axes take over from the keyboard.
	if is_mapped("steer"):
		# A wheel: one degree turned is one degree of the car's wheel.
		steer = -_calibrated_centered("steer")
		if sim_bridge != null:
			steer = sim_bridge.matched_steer(steer)
	elif pad >= 0:
		var x := Input.get_joy_axis(pad, JOY_AXIS_LEFT_X)
		if absf(x) > 0.08:
			steer = -signf(x) * pow((absf(x) - 0.08) / 0.92, 1.6)
	if is_mapped("throttle"):
		throttle = _calibrated_pedal("throttle")
	elif pad >= 0:
		throttle = maxf(throttle, Input.get_joy_axis(pad, JOY_AXIS_TRIGGER_RIGHT))
	if is_mapped("brake"):
		brake = _calibrated_pedal("brake")
	elif pad >= 0:
		brake = maxf(brake, Input.get_joy_axis(pad, JOY_AXIS_TRIGGER_LEFT))
	if is_mapped("clutch"):
		clutch = _calibrated_pedal("clutch")
	if is_mapped("handbrake"):
		handbrake = _calibrated_pedal("handbrake")
	if _button("handbrake_button"):
		handbrake = 1.0

	var bits := 0
	var up := Input.is_physical_key_pressed(KEY_E) or Input.is_physical_key_pressed(KEY_PAGEUP) or _button("shift_up")
	var down := Input.is_physical_key_pressed(KEY_Q) or Input.is_physical_key_pressed(KEY_PAGEDOWN) or _button("shift_down")
	var reset := Input.is_physical_key_pressed(KEY_R) or _button("reset")
	if pad >= 0 and buttons["shift_up"].device < 0:
		up = up or Input.is_joy_button_pressed(pad, JOY_BUTTON_RIGHT_SHOULDER)
		down = down or Input.is_joy_button_pressed(pad, JOY_BUTTON_LEFT_SHOULDER)
		reset = reset or Input.is_joy_button_pressed(pad, JOY_BUTTON_BACK)
		if Input.is_joy_button_pressed(pad, JOY_BUTTON_A):
			handbrake = 1.0
	if up:
		bits |= BUTTON_SHIFT_UP
	if down:
		bits |= BUTTON_SHIFT_DOWN
	if reset:
		bits |= BUTTON_RESET
	if Input.is_physical_key_pressed(KEY_I) or _button("starter"):
		bits |= BUTTON_STARTER

	# H-pattern: the lever's gate, or neutral between gates.
	var gear := NO_GEAR
	if shift_mode == "h_pattern":
		var any_gate := false
		gear = 0
		for gate in GEAR_GATES:
			if buttons[gate].device >= 0:
				any_gate = true
				if _button(gate):
					gear = GEAR_GATES[gate]
		if not any_gate:
			gear = NO_GEAR

	return {
		"steer": clampf(steer, -1.0, 1.0),
		"throttle": clampf(throttle, 0.0, 1.0),
		"brake": clampf(brake, 0.0, 1.0),
		"clutch": clampf(clutch, 0.0, 1.0),
		"handbrake": clampf(handbrake, 0.0, 1.0),
		"buttons": bits,
		"gear": gear,
	}
