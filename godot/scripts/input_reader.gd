# SPDX-License-Identifier: Apache-2.0
# Reads keyboard, gamepad and wheel input and turns it into the values the
# simulation expects. Mapping and calibration are stored in user://input.cfg.
# This is input handling only; nothing here affects how the car behaves
# beyond the values it sends.
extends Node

const CONFIG_PATH := "user://input.cfg"
const AXIS_ACTIONS := ["steer", "throttle", "brake", "clutch", "handbrake"]
const BUTTON_ACTIONS := ["shift_up", "shift_down", "reset", "handbrake_button"]

# Bit values defined by the simulation core (qts_core::input::buttons).
const BUTTON_SHIFT_UP := 1
const BUTTON_SHIFT_DOWN := 2
const BUTTON_RESET := 4

# Each axis mapping: device (-1 = unmapped), axis index, calibrated minimum,
# centre and maximum of the raw value, and whether to flip it.
var axes := {}
# Each button mapping: device (-1 = unmapped) and button index.
var buttons := {}
var keyboard_steer := 0.0
var keyboard_throttle := 0.0
var keyboard_brake := 0.0


func _ready() -> void:
	for action in AXIS_ACTIONS:
		axes[action] = {"device": -1, "axis": 0, "min": -1.0, "center": 0.0, "max": 1.0, "invert": false}
	for action in BUTTON_ACTIONS:
		buttons[action] = {"device": -1, "button": 0}
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


func save_config() -> void:
	var cfg := ConfigFile.new()
	for action in AXIS_ACTIONS:
		for key in axes[action].keys():
			cfg.set_value("axis_" + action, key, axes[action][key])
	for action in BUTTON_ACTIONS:
		for key in buttons[action].keys():
			cfg.set_value("button_" + action, key, buttons[action][key])
	cfg.save(CONFIG_PATH)


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
	return 1.0 - value if m.invert else value


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
		steer = -_calibrated_centered("steer")
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

	return {
		"steer": clampf(steer, -1.0, 1.0),
		"throttle": clampf(throttle, 0.0, 1.0),
		"brake": clampf(brake, 0.0, 1.0),
		"clutch": clampf(clutch, 0.0, 1.0),
		"handbrake": clampf(handbrake, 0.0, 1.0),
		"buttons": bits,
	}
