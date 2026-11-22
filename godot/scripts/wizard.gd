# SPDX-License-Identifier: Apache-2.0
# First-run wheel and pedal wizard (D-085). Step by step: find the wheel,
# its centre and rotation; find and calibrate each pedal; set the brake
# curve and deadzones; set force-feedback strength from the wheel-base
# model in N·m; save everything as a profile for the device. Controls are
# read as the core reads them on the physics thread (input_scan), and the
# result is the same mapping the Controls screen edits.
extends CanvasLayer

signal closed

const STEPS := ["welcome", "wheel", "centre", "rotation", "throttle", "brake", "clutch", "shaping", "ffb", "done"]
const PEDALS := {"throttle": "accelerator", "brake": "brake pedal", "clutch": "clutch pedal"}
# Column torque of hard cornering (qts-sim ffb-gain, D-083).
const HARD_CORNER_NM := 17.0
const KNEE := 0.75
# How far an axis must move to count as the one being used.
const DETECT_CHANGE := 0.4

var sim
var reader
var settings
var step := 0
var scan := {}
var baseline := {}
var device := ""
var device_name := ""
var steer_axis := -1
var centre := 0.0
var rotation_deg := 900.0
var pedal_rest := {}
var pedal_far := {}
var pedal_axis := {}
var models: Array = []
var model_index := -1
var used_axes := {}

var title: Label
var body: Label
var live: Label
var controls: VBoxContainer
var next_button: Button


func setup(bridge, input_reader, store) -> void:
	sim = bridge
	reader = input_reader
	settings = store


func _ready() -> void:
	layer = 25
	var shade := ColorRect.new()
	shade.color = Color(0.03, 0.05, 0.08, 0.96)
	shade.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(shade)
	var margin := MarginContainer.new()
	margin.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	for side in ["left", "right", "top", "bottom"]:
		margin.add_theme_constant_override("margin_" + side, 60)
	add_child(margin)
	var box := VBoxContainer.new()
	box.add_theme_constant_override("separation", 16)
	margin.add_child(box)
	title = Label.new()
	title.add_theme_font_size_override("font_size", 30)
	box.add_child(title)
	body = Label.new()
	body.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	body.add_theme_font_size_override("font_size", 18)
	box.add_child(body)
	live = Label.new()
	live.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	live.add_theme_font_size_override("font_size", 18)
	live.add_theme_color_override("font_color", Color(1.0, 0.85, 0.4))
	box.add_child(live)
	controls = VBoxContainer.new()
	controls.add_theme_constant_override("separation", 8)
	box.add_child(controls)
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 12)
	box.add_child(row)
	var skip := Button.new()
	skip.text = "Skip the wizard"
	skip.pressed.connect(finish.bind(false))
	row.add_child(skip)
	var back := Button.new()
	back.text = "Back"
	back.pressed.connect(func() -> void: _go(maxi(step - 1, 0)))
	row.add_child(back)
	next_button = Button.new()
	next_button.text = "Next"
	next_button.custom_minimum_size = Vector2(160, 40)
	next_button.pressed.connect(_next)
	row.add_child(next_button)
	models = sim.wheel_models()
	_go(0)


func _refresh_scan() -> void:
	scan.clear()
	for d in sim.input_scan():
		scan[d.id] = d


func _take_baseline() -> void:
	_refresh_scan()
	baseline.clear()
	for id in scan:
		baseline[id] = scan[id].axes.duplicate()


# The axis that has moved furthest since the baseline, as [change, id, axis].
func _moved_axis(only_device: String = "") -> Array:
	var best := [0.0, "", -1]
	for id in scan:
		if only_device != "" and id != only_device:
			continue
		if not baseline.has(id):
			continue
		var axes: PackedFloat32Array = scan[id].axes
		for a in range(axes.size()):
			if used_axes.has("%s:%d" % [id, a]):
				continue
			var change := absf(axes[a] - baseline[id][a])
			if change > best[0]:
				best = [change, id, a]
	return best


func _raw(id: String, axis: int) -> float:
	if not scan.has(id):
		return 0.0
	var axes: PackedFloat32Array = scan[id].axes
	return axes[axis] if axis >= 0 and axis < axes.size() else 0.0


# Rotation lock to lock, degrees, from the wheel turned a known angle:
# the device reports its whole rotation as -1..1, so a quarter turn moves
# the axis by 90 / (rotation / 2).
static func rotation_from_quarter_turn(centre_raw: float, turned_raw: float) -> float:
	var moved := absf(turned_raw - centre_raw)
	if moved < 0.01:
		return 0.0
	return 2.0 * 90.0 / moved


# Default force-feedback gain for a base: hard cornering at the knee.
static func gain_for(peak_nm: float) -> float:
	return minf(KNEE * peak_nm / HARD_CORNER_NM, 1.0)


func _clear_controls() -> void:
	for c in controls.get_children():
		c.queue_free()


func _go(index: int) -> void:
	step = clampi(index, 0, STEPS.size() - 1)
	_clear_controls()
	next_button.text = "Next"
	next_button.disabled = false
	match STEPS[step]:
		"welcome":
			title.text = "Set up your wheel and pedals"
			body.text = "This takes about two minutes: the wheel and its rotation, each pedal, the brake feel, and force feedback in real units. You can change all of it later on the Controls screen. Plug everything in and close the base's own software if it locks the device."
			_refresh_scan()
			var names: Array = []
			for id in scan:
				names.append(scan[id].name)
			live.text = "Found: " + (", ".join(names) if not names.is_empty() else "no controllers (you can still drive with the keyboard)")
		"wheel":
			title.text = "1. Your wheel"
			body.text = "Turn the steering wheel a little either way, then back to the middle."
			live.text = "waiting for the wheel to move"
			next_button.disabled = true
			used_axes.clear()
			_take_baseline()
		"centre":
			title.text = "2. Centre"
			body.text = "Hold the wheel straight ahead, with the rim level, and press Next."
		"rotation":
			title.text = "3. Rotation"
			body.text = "Turn the wheel exactly a quarter turn to the right (the top of the rim at three o'clock), hold it there and press Next. This measures the rotation your base is set to, so the car's steering can match it degree for degree."
		"throttle", "brake", "clutch":
			var action: String = STEPS[step]
			title.text = "%d. The %s" % [step + 1, PEDALS[action]]
			body.text = "Press the %s all the way down, then let it come back. If you have no %s, press Skip this pedal." % [PEDALS[action], PEDALS[action]]
			live.text = "waiting for the %s" % PEDALS[action]
			next_button.disabled = true
			_take_baseline()
			pedal_axis.erase(action)
			var skip := Button.new()
			skip.text = "Skip this pedal"
			skip.pressed.connect(func() -> void: _go(step + 1))
			controls.add_child(skip)
		"shaping":
			title.text = "%d. Brake feel and deadzones" % (step + 1)
			body.text = "A curve above 1 makes the first part of the brake gentler and the end firmer, closer to a real pedal on a load cell. Deadzones ignore a little travel at each end so a pedal at rest reads zero and a pedal at the floor reads full."
			_shape_slider("brake", "curve", "Brake curve", 1.0, 3.0, 0.1, 1.5)
			for action in pedal_axis.keys():
				_shape_slider(action, "deadzone_low", "%s deadzone at rest (%%)" % PEDALS[action].capitalize(), 0.0, 15.0, 0.5, 2.0, 100.0)
				_shape_slider(action, "deadzone_high", "%s deadzone at the floor (%%)" % PEDALS[action].capitalize(), 0.0, 15.0, 0.5, 2.0, 100.0)
		"ffb":
			title.text = "%d. Force feedback" % (step + 1)
			body.text = "Pick your wheel base: its peak torque sets the strength in N·m. The suggested gain puts hard cornering (about %.0f N·m at the car's steering column) at three quarters of the base's force, so fast corners do not clip; gain 1 would be the car's real torque." % HARD_CORNER_NM
			var pick := OptionButton.new()
			for m in models:
				pick.add_item("%s%s" % [m.name, "" if m.verified else " (unverified)"])
			pick.select(model_index)
			pick.item_selected.connect(func(i: int) -> void:
				model_index = i
				_show_ffb())
			controls.add_child(pick)
			_show_ffb()
		"done":
			title.text = "Done"
			var lines: Array = []
			lines.append("Wheel: %s, rotation %.0f degrees" % [device_name, rotation_deg] if device != "" else "Wheel: none (keyboard or gamepad)")
			for action in PEDALS:
				lines.append("%s: %s" % [PEDALS[action].capitalize(), "axis %d" % pedal_axis[action] if pedal_axis.has(action) else "not set"])
			if model_index >= 0:
				lines.append("Force feedback: %s, %.1f N·m, gain %.2f" % [models[model_index].name, models[model_index].peak_torque_nm, gain_for(models[model_index].peak_torque_nm)])
			body.text = "\n".join(lines) + "\n\nThis is saved as the profile for this wheel, and loaded again whenever the same wheel is found."
			next_button.text = "Save and finish"


func _shape_slider(action: String, key: String, label_text: String, lo: float, hi: float, step_v: float, value: float, scale: float = 1.0) -> void:
	if not pedal_axis.has(action):
		return
	var row := HBoxContainer.new()
	var label := Label.new()
	label.custom_minimum_size = Vector2(380, 0)
	row.add_child(label)
	var s := HSlider.new()
	s.min_value = lo
	s.max_value = hi
	s.step = step_v
	s.custom_minimum_size = Vector2(300, 0)
	s.value = reader.direct[action].get(key, value / scale) * scale
	var update := func(v: float) -> void:
		label.text = "%s: %s" % [label_text, str(snappedf(v, step_v))]
		reader.direct[action][key] = v / scale
	s.value_changed.connect(update)
	update.call(s.value)
	row.add_child(s)
	controls.add_child(row)


func _show_ffb() -> void:
	if model_index < 0:
		live.text = "choose a wheel base"
		return
	var m: Dictionary = models[model_index]
	var g := gain_for(m.peak_torque_nm)
	live.text = "%.1f N·m%s: suggested gain %.2f (hard cornering %.1f N·m at your wheel). %s" % [
		m.peak_torque_nm, "" if m.verified else " (unverified figure)", g, HARD_CORNER_NM * g, m.note]


func _process(_delta: float) -> void:
	if step >= STEPS.size():
		return
	match STEPS[step]:
		"wheel":
			_refresh_scan()
			var best := _moved_axis()
			if best[0] > DETECT_CHANGE:
				device = best[1]
				steer_axis = best[2]
				device_name = scan[device].name
				live.text = "Found: %s, axis %d" % [device_name, steer_axis]
				next_button.disabled = false
				var extra: Dictionary = reader.load_profile(device_name)
				if extra.get("found", false):
					live.text += "\nA saved profile for this wheel was loaded; Next to check it, or Skip the wizard to keep it."
					_apply_ffb_extra(extra)
		"centre", "rotation":
			_refresh_scan()
			live.text = "wheel axis %+.3f" % _raw(device, steer_axis)
		"throttle", "brake", "clutch":
			_refresh_scan()
			var action: String = STEPS[step]
			if not pedal_axis.has(action):
				var best := _moved_axis()
				if best[0] > DETECT_CHANGE and not (best[1] == device and best[2] == steer_axis):
					pedal_axis[action] = best[2]
					pedal_rest[action] = baseline[best[1]][best[2]]
					pedal_far[action] = _raw(best[1], best[2])
					reader.direct[action]["device"] = best[1]
					reader.direct[action]["axis"] = best[2]
					used_axes["%s:%d" % [best[1], best[2]]] = true
			if pedal_axis.has(action):
				var id: String = reader.direct[action].device
				var raw := _raw(id, pedal_axis[action])
				# Furthest from rest seen so far is the floor.
				if absf(raw - pedal_rest[action]) > absf(pedal_far[action] - pedal_rest[action]):
					pedal_far[action] = raw
				reader.direct[action]["min"] = pedal_rest[action]
				reader.direct[action]["max"] = pedal_far[action]
				reader.direct[action]["invert"] = false
				live.text = "%s on axis %d: %.0f%%" % [PEDALS[action].capitalize(), pedal_axis[action], reader.direct_calibrated(action, raw) * 100.0]
				next_button.disabled = absf(pedal_far[action] - pedal_rest[action]) < 0.5
		"shaping":
			var parts: Array = []
			for action in pedal_axis.keys():
				var id: String = reader.direct[action].device
				parts.append("%s %.0f%%" % [PEDALS[action], reader.direct_calibrated(action, _raw(id, pedal_axis[action])) * 100.0])
			live.text = "   ".join(parts)


func _next() -> void:
	match STEPS[step]:
		"centre":
			centre = _raw(device, steer_axis)
		"rotation":
			var r := rotation_from_quarter_turn(centre, _raw(device, steer_axis))
			if r < 180.0 or r > 3000.0:
				live.text = "That did not look like a quarter turn (%.0f degrees of rotation); turn exactly to three o'clock and try again." % r
				return
			rotation_deg = snappedf(r, 10.0)
			# The whole device range, centred: the calibrated value is then
			# the fraction of the base's rotation, which the car matches.
			reader.direct["steer"] = {"device": device, "axis": steer_axis, "min": centre - 1.0, "center": centre, "max": centre + 1.0,
				"invert": false, "deadzone_low": 0.0, "deadzone_high": 0.0, "curve": 1.0}
			used_axes["%s:%d" % [device, steer_axis]] = true
		"done":
			finish(true)
			return
	if STEPS[step] == "wheel" and device == "":
		return
	if STEPS[step] == "welcome" and scan.is_empty():
		_go(STEPS.find("ffb"))
		return
	_go(step + 1)


func _apply_ffb_extra(extra: Dictionary) -> void:
	if extra.has("rotation_deg"):
		rotation_deg = extra.rotation_deg
	for i in models.size():
		if models[i].id == extra.get("wheel_model", ""):
			model_index = i


# Saves (or, skipping, just closes) and marks the wizard as done.
func finish(save: bool) -> void:
	if save:
		reader.use_direct = true
		reader.save_config()
		var preset: String = sim.ffb_config().get("id", "")
		sim.ffb_set("device_rotation_deg", rotation_deg)
		settings.set_ffb(preset, "device_rotation_deg", rotation_deg)
		var extra := {"rotation_deg": rotation_deg}
		if model_index >= 0:
			var m: Dictionary = models[model_index]
			var g := gain_for(m.peak_torque_nm)
			sim.ffb_set("device_max_torque_nm", m.peak_torque_nm)
			sim.ffb_set("gain", g)
			settings.set_ffb(preset, "device_max_torque_nm", m.peak_torque_nm)
			settings.set_ffb(preset, "gain", g)
			settings.set_value("ffb/wheel_model", m.id)
			extra["wheel_model"] = m.id
			extra["gain"] = g
		if device_name != "":
			reader.save_profile(device_name, extra)
	settings.set_value("input/wizard_done", true)
	settings.save()
	sim.input_scan_close()
	closed.emit()
