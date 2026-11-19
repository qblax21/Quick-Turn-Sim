# SPDX-License-Identifier: Apache-2.0
# Controls and wheel setup: assign axes and buttons, calibrate their range,
# and adjust the force-feedback settings.
extends CanvasLayer

signal closed

const AXIS_LABELS := {
	"steer": "Steering",
	"throttle": "Accelerator",
	"brake": "Brake",
	"clutch": "Clutch",
	"handbrake": "Handbrake (axis)",
}
const BUTTON_LABELS := {
	"shift_up": "Shift up",
	"shift_down": "Shift down",
	"reset": "Reset car",
	"handbrake_button": "Handbrake (button)",
}
const FFB_SLIDERS := [
	["gain", "Gain", 0.0, 1.5, 0.01],
	["device_max_torque_nm", "Wheel peak torque (N·m)", 1.0, 30.0, 0.1],
	["lowpass_cutoff_hz", "Low-pass cutoff (Hz, 0 = off)", 0.0, 200.0, 1.0],
	["slew_limit_nm_per_s", "Slew limit (N·m/s, 0 = off)", 0.0, 3000.0, 10.0],
	["soft_clip_knee", "Soft-clip knee", 0.3, 0.95, 0.01],
	["min_force", "Minimum force (weak wheels)", 0.0, 0.3, 0.005],
	["damping_nm_s_per_rad", "Damping", 0.0, 0.5, 0.005],
	["friction_nm", "Friction (N·m)", 0.0, 2.0, 0.01],
	["inertia_nm_s2_per_rad", "Inertia", 0.0, 0.05, 0.001],
	["road_detail_gain_nm_per_n", "Road detail", 0.0, 0.003, 0.0001],
	["kerb_gain_nm_per_n", "Kerb detail", 0.0, 0.01, 0.0001],
]

var sim
var reader
var settings
var preset_id := ""
var status: Label
var rows := {}           # action -> {"value": Label, "bar": ProgressBar}
var listening := ""      # action waiting for an axis or button
var listen_is_axis := true
var baseline := {}       # device -> [axis values] when listening started
var calibrating := ""    # axis action being calibrated
var cal_min := 0.0
var cal_max := 0.0
# Devices read directly by the core: id -> {"name", "axes", "buttons"}.
var scan := {}
var direct_baseline := {}  # id -> axes when listening started
var direct_held := {}      # id -> buttons held when listening started
var cal_dmin := 0.0
var cal_dmax := 0.0


func setup(bridge, input_reader, store) -> void:
	sim = bridge
	reader = input_reader
	settings = store


func _ready() -> void:
	var panel := PanelContainer.new()
	panel.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT, Control.PRESET_MODE_MINSIZE, 30)
	add_child(panel)
	var margin := MarginContainer.new()
	for side in ["left", "right", "top", "bottom"]:
		margin.add_theme_constant_override("margin_" + side, 18)
	panel.add_child(margin)
	var columns := HBoxContainer.new()
	columns.add_theme_constant_override("separation", 40)
	margin.add_child(columns)

	# Left column: mapping and calibration.
	var left := VBoxContainer.new()
	left.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	left.add_theme_constant_override("separation", 6)
	columns.add_child(left)
	_heading(left, "Controls")
	var help := Label.new()
	help.text = "Assign: press the button, then move the control fully (or press the button on the device).\nCalibrate: press, move the control through its whole range, then press Done. For steering, finish with the wheel centred.\nUnassigned controls fall back to the keyboard and the first gamepad."
	help.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	help.add_theme_font_size_override("font_size", 13)
	left.add_child(help)
	var direct_box := CheckBox.new()
	direct_box.text = "Read wheel, pedals and shifter on the physics thread (recommended)"
	direct_box.button_pressed = reader.use_direct
	direct_box.toggled.connect(func(on: bool) -> void: reader.use_direct = on)
	left.add_child(direct_box)
	_refresh_scan()
	for action in reader.AXIS_ACTIONS:
		_axis_row(left, action)
	left.add_child(HSeparator.new())
	for action in reader.BUTTON_ACTIONS:
		_button_row(left, action)
	status = Label.new()
	status.add_theme_color_override("font_color", Color(1.0, 0.85, 0.4))
	left.add_child(status)
	var pads := Label.new()
	var names: Array = []
	for pad in Input.get_connected_joypads():
		names.append("%d: %s" % [pad, Input.get_joy_name(pad)])
	pads.text = "Connected devices: " + (", ".join(names) if names.size() > 0 else "none")
	pads.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	left.add_child(pads)

	# Right column: force feedback.
	var right := VBoxContainer.new()
	right.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	right.add_theme_constant_override("separation", 6)
	columns.add_child(right)
	_heading(right, "Force feedback")
	var config: Dictionary = sim.ffb_config()
	if config.is_empty():
		var presets: PackedStringArray = sim.list_ids("ffb")
		if presets.size() > 0:
			sim.ffb_load_preset(presets[0])
			settings.apply_ffb(sim)
			config = sim.ffb_config()
	preset_id = config.get("id", "")
	var preset_label := Label.new()
	preset_label.text = "Preset: %s%s" % [config.get("name", "none"), "" if config.get("tuned", false) else "\nThis preset has not been tuned on hardware. Start with low gain."]
	preset_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	right.add_child(preset_label)
	var invert := CheckBox.new()
	invert.text = "Invert force direction"
	invert.button_pressed = config.get("invert", false)
	invert.toggled.connect(func(on: bool) -> void:
		sim.ffb_set("invert", 1.0 if on else 0.0)
		settings.set_ffb(preset_id, "invert", 1.0 if on else 0.0))
	right.add_child(invert)
	var sign_help := Label.new()
	sign_help.text = "Sign check: drive slowly and turn. The wheel should resist and return to centre when released. If it pulls further into the turn, tick Invert."
	sign_help.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	sign_help.add_theme_font_size_override("font_size", 13)
	right.add_child(sign_help)
	for entry in FFB_SLIDERS:
		_ffb_slider(right, entry, config)
	var note := Label.new()
	note.text = "Changes apply at once and are remembered for this preset when you save. The preset file in data/ffb is not changed."
	note.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	note.add_theme_font_size_override("font_size", 13)
	right.add_child(note)

	var done := Button.new()
	done.text = "Save and return"
	done.custom_minimum_size = Vector2(200, 44)
	done.pressed.connect(func() -> void:
		reader.save_config()
		settings.save()
		sim.input_scan_close()
		closed.emit())
	right.add_child(done)


func _heading(parent: Control, text: String) -> void:
	var label := Label.new()
	label.text = text
	label.add_theme_font_size_override("font_size", 22)
	parent.add_child(label)


func _axis_row(parent: Control, action: String) -> void:
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 8)
	var label := Label.new()
	label.text = AXIS_LABELS[action]
	label.custom_minimum_size = Vector2(150, 0)
	row.add_child(label)
	var value := Label.new()
	value.custom_minimum_size = Vector2(170, 0)
	row.add_child(value)
	var bar := ProgressBar.new()
	bar.min_value = -1.0 if action == "steer" else 0.0
	bar.max_value = 1.0
	bar.step = 0.001
	bar.show_percentage = false
	bar.custom_minimum_size = Vector2(140, 0)
	row.add_child(bar)
	var assign := Button.new()
	assign.text = "Assign"
	assign.pressed.connect(func() -> void: _listen(action, true))
	row.add_child(assign)
	var calibrate := Button.new()
	calibrate.text = "Calibrate"
	calibrate.pressed.connect(func() -> void: _toggle_calibration(action, calibrate))
	row.add_child(calibrate)
	var invert := CheckBox.new()
	invert.text = "Invert"
	invert.button_pressed = reader.axes[action].invert
	invert.toggled.connect(func(on: bool) -> void:
		reader.axes[action].invert = on
		reader.direct[action].invert = on)
	row.add_child(invert)
	var clear := Button.new()
	clear.text = "Clear"
	clear.pressed.connect(func() -> void:
		reader.axes[action].device = -1
		reader.direct[action].device = "")
	row.add_child(clear)
	parent.add_child(row)
	rows[action] = {"value": value, "bar": bar}


func _button_row(parent: Control, action: String) -> void:
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 8)
	var label := Label.new()
	label.text = BUTTON_LABELS[action]
	label.custom_minimum_size = Vector2(150, 0)
	row.add_child(label)
	var value := Label.new()
	value.custom_minimum_size = Vector2(170, 0)
	row.add_child(value)
	var assign := Button.new()
	assign.text = "Assign"
	assign.pressed.connect(func() -> void: _listen(action, false))
	row.add_child(assign)
	var clear := Button.new()
	clear.text = "Clear"
	clear.pressed.connect(func() -> void:
		reader.buttons[action].device = -1
		reader.direct[action].device = "")
	row.add_child(clear)
	parent.add_child(row)
	rows[action] = {"value": value}


func _ffb_slider(parent: Control, entry: Array, config: Dictionary) -> void:
	var key: String = entry[0]
	var row := HBoxContainer.new()
	var label := Label.new()
	label.text = entry[1]
	label.custom_minimum_size = Vector2(230, 0)
	row.add_child(label)
	var slider := HSlider.new()
	slider.min_value = entry[2]
	slider.max_value = entry[3]
	slider.step = entry[4]
	slider.value = config.get(key, entry[2])
	slider.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	slider.custom_minimum_size = Vector2(200, 0)
	row.add_child(slider)
	var readout := Label.new()
	readout.custom_minimum_size = Vector2(80, 0)
	readout.text = str(snappedf(slider.value, entry[4]))
	row.add_child(readout)
	slider.value_changed.connect(func(v: float) -> void:
		readout.text = str(snappedf(v, entry[4]))
		if sim.ffb_set(key, v):
			settings.set_ffb(preset_id, key, v)
		else:
			status.text = sim.last_error())
	parent.add_child(row)


func _snapshot_axes() -> Dictionary:
	var snap := {}
	for pad in Input.get_connected_joypads():
		var values: Array = []
		for axis in range(JOY_AXIS_MAX):
			values.append(Input.get_joy_axis(pad, axis))
		snap[pad] = values
	return snap


func _refresh_scan() -> void:
	scan.clear()
	for d in sim.input_scan():
		scan[d.id] = d


func _direct_raw(action: String) -> float:
	var m: Dictionary = reader.direct[action]
	if m.device == "" or not scan.has(m.device):
		return 0.0
	var axes: PackedFloat32Array = scan[m.device].axes
	return axes[m.axis] if m.axis < axes.size() else 0.0


func _direct_pressed(action: String) -> bool:
	var m: Dictionary = reader.direct[action]
	return m.device != "" and scan.has(m.device) and m.button in scan[m.device].buttons


func _listen(action: String, is_axis: bool) -> void:
	listening = action
	listen_is_axis = is_axis
	baseline = _snapshot_axes()
	direct_baseline.clear()
	direct_held.clear()
	for id in scan:
		direct_baseline[id] = scan[id].axes.duplicate()
		direct_held[id] = scan[id].buttons.duplicate()
	status.text = "Waiting for input for: %s  (Esc to cancel)" % (AXIS_LABELS.get(action, BUTTON_LABELS.get(action, action)))


func _toggle_calibration(action: String, button: Button) -> void:
	var mapped: bool = reader.axes[action].device >= 0 or reader.has_direct(action)
	if calibrating == action:
		# Finish: the current position is the centre (matters for steering).
		var m: Dictionary = reader.axes[action]
		if m.device >= 0:
			m.min = cal_min
			m.max = cal_max
			m.center = reader.raw_axis(action) if action == "steer" else cal_min
		var d: Dictionary = reader.direct[action]
		if d.device != "":
			d.min = cal_dmin
			d.max = cal_dmax
			d.center = _direct_raw(action) if action == "steer" else cal_dmin
		calibrating = ""
		button.text = "Calibrate"
		status.text = "Calibrated %s." % AXIS_LABELS[action]
	elif calibrating == "" and mapped:
		calibrating = action
		cal_min = reader.raw_axis(action)
		cal_max = cal_min
		cal_dmin = _direct_raw(action)
		cal_dmax = cal_dmin
		button.text = "Done"
		status.text = "Move %s through its whole range, then press Done." % AXIS_LABELS[action]
	elif not mapped:
		status.text = "Assign %s before calibrating it." % AXIS_LABELS[action]


func _process(_delta: float) -> void:
	_refresh_scan()
	if listening != "":
		if Input.is_physical_key_pressed(KEY_ESCAPE):
			listening = ""
			status.text = "Cancelled."
		elif listen_is_axis:
			_detect_axis()
		else:
			_detect_button()
	if calibrating != "":
		var raw: float = reader.raw_axis(calibrating)
		cal_min = minf(cal_min, raw)
		cal_max = maxf(cal_max, raw)
		var draw: float = _direct_raw(calibrating)
		cal_dmin = minf(cal_dmin, draw)
		cal_dmax = maxf(cal_dmax, draw)
	for action in reader.AXIS_ACTIONS:
		var m: Dictionary = reader.axes[action]
		var row: Dictionary = rows[action]
		if reader.has_direct(action):
			var d: Dictionary = reader.direct[action]
			var raw_d := _direct_raw(action)
			var device_name: String = scan[d.device].name if scan.has(d.device) else "missing device"
			row.value.text = "%s %s  raw %+.2f" % [device_name.left(14), sim.input_axis_name(d.axis), raw_d]
			row.bar.value = reader.direct_calibrated(action, raw_d)
		elif m.device < 0:
			row.value.text = "keyboard / gamepad"
			row.bar.value = 0.0
		else:
			row.value.text = "device %d axis %d  raw %+.2f" % [m.device, m.axis, reader.raw_axis(action)]
			row.bar.value = reader._calibrated_centered(action) if action == "steer" else reader._calibrated_pedal(action)
	for action in reader.BUTTON_ACTIONS:
		var b: Dictionary = reader.buttons[action]
		if reader.has_direct(action):
			var d: Dictionary = reader.direct[action]
			var device_name: String = scan[d.device].name if scan.has(d.device) else "missing device"
			rows[action].value.text = "%s button %d%s" % [device_name.left(14), d.button, "  (pressed)" if _direct_pressed(action) else ""]
			continue
		rows[action].value.text = "not assigned" if b.device < 0 else "device %d button %d%s" % [b.device, b.button, "  (pressed)" if reader._button(action) else ""]


func _detect_axis() -> void:
	# The axis that has moved furthest from where it was when listening
	# began, both as Godot sees the devices and as the core reads them.
	var best_pad := [0.0, -1, 0]
	for pad in Input.get_connected_joypads():
		if not baseline.has(pad):
			continue
		for axis in range(JOY_AXIS_MAX):
			var change := absf(Input.get_joy_axis(pad, axis) - baseline[pad][axis])
			if change > best_pad[0]:
				best_pad = [change, pad, axis]
	var best_direct := [0.0, "", 0]
	for id in scan:
		if not direct_baseline.has(id):
			continue
		var axes: PackedFloat32Array = scan[id].axes
		for axis in range(axes.size()):
			var change := absf(axes[axis] - direct_baseline[id][axis])
			if change > best_direct[0]:
				best_direct = [change, id, axis]
	if maxf(best_pad[0], best_direct[0]) < 0.5:
		return
	# Map every way the movement was seen; the core reads the device itself
	# and compares with what Godot read to measure the delay.
	var said: Array = []
	if best_pad[0] > 0.3:
		var m: Dictionary = reader.axes[listening]
		m.device = best_pad[1]
		m.axis = best_pad[2]
		m.min = -1.0
		m.center = 0.0 if listening == "steer" else -1.0
		m.max = 1.0
		said.append("Godot device %d axis %d" % [m.device, m.axis])
	if best_direct[0] > 0.3:
		var d: Dictionary = reader.direct[listening]
		d.device = best_direct[1]
		d.axis = best_direct[2]
		d.min = -1.0
		d.center = 0.0 if listening == "steer" else -1.0
		d.max = 1.0
		said.append("%s %s, read on the physics thread" % [scan[d.device].name, sim.input_axis_name(d.axis)])
	status.text = "Assigned %s to %s. Calibrate it next." % [AXIS_LABELS[listening], " and ".join(said)]
	listening = ""


func _detect_button() -> void:
	for id in scan:
		for button in scan[id].buttons:
			if direct_held.has(id) and button in direct_held[id]:
				continue
			var d: Dictionary = reader.direct[listening]
			d.device = id
			d.button = button
			# Godot's view of the same press, for keyboard-free fallback.
			for pad in Input.get_connected_joypads():
				for b in range(JOY_BUTTON_MAX):
					if Input.is_joy_button_pressed(pad, b):
						reader.buttons[listening].device = pad
						reader.buttons[listening].button = b
			status.text = "Assigned %s to %s button %d." % [BUTTON_LABELS[listening], scan[id].name, button]
			listening = ""
			return
	for pad in Input.get_connected_joypads():
		for button in range(JOY_BUTTON_MAX):
			if Input.is_joy_button_pressed(pad, button):
				reader.buttons[listening].device = pad
				reader.buttons[listening].button = button
				status.text = "Assigned %s to device %d button %d." % [BUTTON_LABELS[listening], pad, button]
				listening = ""
				return
