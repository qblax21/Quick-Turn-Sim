# SPDX-License-Identifier: Apache-2.0
# Start menu: pick a car, a track, assists and a wheel, then drive.
extends CanvasLayer

signal drive_requested(car_id, track_id, auto_clutch, auto_shift, ffb_preset, device_id, direct_steering)
signal input_setup_requested
signal settings_requested

var sim
var settings
var car_list: OptionButton
var track_list: OptionButton
var preset_list: OptionButton
var device_list: OptionButton
var auto_clutch: CheckBox
var auto_shift: CheckBox
var direct_steering: CheckBox
var description: Label
var error_label: Label
var car_ids: PackedStringArray
var track_ids: PackedStringArray
var preset_ids: PackedStringArray
var device_ids: Array = []


func setup(bridge, store) -> void:
	sim = bridge
	settings = store


func _ready() -> void:
	# A full-screen container keeps the panel centred at any window size.
	var center := CenterContainer.new()
	center.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(center)
	var panel := PanelContainer.new()
	panel.custom_minimum_size = Vector2(620, 0)
	center.add_child(panel)
	var margin := MarginContainer.new()
	for side in ["left", "right", "top", "bottom"]:
		margin.add_theme_constant_override("margin_" + side, 24)
	panel.add_child(margin)
	var box := VBoxContainer.new()
	box.add_theme_constant_override("separation", 10)
	margin.add_child(box)

	var title := Label.new()
	title.text = "%s  %s" % [sim.product_name(), sim.version()]
	title.add_theme_font_size_override("font_size", 28)
	box.add_child(title)
	var notice := Label.new()
	notice.text = "Foundation build. Cars are fictional and every vehicle, tire and surface parameter is a placeholder."
	notice.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	box.add_child(notice)

	car_ids = sim.list_ids("cars")
	car_list = _add_choice(box, "Car", car_ids, "cars")
	track_ids = sim.list_ids("tracks")
	track_list = _add_choice(box, "Track", track_ids, "tracks")
	description = Label.new()
	description.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	box.add_child(description)
	_restore(car_list, car_ids, "menu/car")
	_restore(track_list, track_ids, "menu/track")
	car_list.item_selected.connect(func(_i: int) -> void: _describe())
	track_list.item_selected.connect(func(_i: int) -> void: _describe())

	auto_clutch = CheckBox.new()
	auto_clutch.text = "Automatic clutch"
	auto_clutch.button_pressed = settings.get_value("menu/auto_clutch")
	box.add_child(auto_clutch)
	auto_shift = CheckBox.new()
	auto_shift.text = "Automatic gears"
	auto_shift.button_pressed = settings.get_value("menu/auto_shift")
	box.add_child(auto_shift)
	var traction := CheckBox.new()
	traction.text = "Traction control"
	traction.button_pressed = settings.get_value("menu/traction_control")
	traction.toggled.connect(func(on: bool) -> void: settings.set_value("menu/traction_control", on))
	box.add_child(traction)

	box.add_child(HSeparator.new())
	preset_ids = sim.list_ids("ffb")
	preset_list = _add_choice(box, "Force-feedback preset", preset_ids, "ffb")
	_restore(preset_list, preset_ids, "menu/ffb_preset")
	device_list = OptionButton.new()
	device_list.add_item("No force-feedback device")
	device_ids = [""]
	for device in sim.ffb_devices():
		var suffix := "" if device.force_feedback else "  (no force feedback)"
		device_list.add_item("%s%s" % [device.name, suffix])
		device_ids.append(device.id)
		# Preselect the first wheel that supports force feedback.
		if device.force_feedback and device_list.selected == 0:
			device_list.select(device_ids.size() - 1)
	_add_row(box, "Wheel", device_list)
	direct_steering = CheckBox.new()
	direct_steering.text = "Read steering straight from the wheel at the physics rate (untested)"
	box.add_child(direct_steering)

	box.add_child(HSeparator.new())
	var buttons := HBoxContainer.new()
	buttons.add_theme_constant_override("separation", 12)
	box.add_child(buttons)
	var drive := Button.new()
	drive.text = "Drive"
	drive.custom_minimum_size = Vector2(160, 44)
	drive.pressed.connect(_on_drive)
	buttons.add_child(drive)
	var setup_button := Button.new()
	setup_button.text = "Controls and wheel setup"
	setup_button.custom_minimum_size = Vector2(0, 44)
	setup_button.pressed.connect(func() -> void: input_setup_requested.emit())
	buttons.add_child(setup_button)
	var settings_button := Button.new()
	settings_button.text = "Settings"
	settings_button.custom_minimum_size = Vector2(0, 44)
	settings_button.pressed.connect(func() -> void: settings_requested.emit())
	buttons.add_child(settings_button)
	var quit := Button.new()
	quit.text = "Quit"
	quit.custom_minimum_size = Vector2(0, 44)
	quit.pressed.connect(func() -> void: get_tree().quit())
	buttons.add_child(quit)

	error_label = Label.new()
	error_label.add_theme_color_override("font_color", Color(1.0, 0.45, 0.4))
	error_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	box.add_child(error_label)

	var keys := Label.new()
	keys.text = "Keyboard: W/S or arrows = pedals, A/D = steer, Space = handbrake, E/Q = shift, Shift = clutch, R = reset.\nWhile driving: Esc = menu, C = seat or chase camera, G = ghost of the best lap, M = mute sound, P = pause, F1 = tire and FFB overlay, F2 = telemetry, F3 = replay."
	keys.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	keys.add_theme_font_size_override("font_size", 13)
	box.add_child(keys)
	_describe()


func _add_row(parent: Control, caption: String, control: Control) -> void:
	var row := HBoxContainer.new()
	var label := Label.new()
	label.text = caption
	label.custom_minimum_size = Vector2(190, 0)
	row.add_child(label)
	control.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	row.add_child(control)
	parent.add_child(row)


func _add_choice(parent: Control, caption: String, ids: PackedStringArray, kind: String) -> OptionButton:
	var list := OptionButton.new()
	for id in ids:
		var info: Dictionary = sim.describe(kind, id)
		list.add_item(info.get("name", id))
	_add_row(parent, caption, list)
	return list


# Selects the entry remembered from the last session, if it still exists.
func _restore(list: OptionButton, ids: PackedStringArray, key: String) -> void:
	var index := ids.find(settings.get_value(key))
	if index >= 0:
		list.select(index)


# Stores the current choices so the menu opens the same way next time.
func remember_choices() -> void:
	settings.set_value("menu/car", _selected(car_ids, car_list))
	settings.set_value("menu/track", _selected(track_ids, track_list))
	settings.set_value("menu/auto_clutch", auto_clutch.button_pressed)
	settings.set_value("menu/auto_shift", auto_shift.button_pressed)
	settings.set_value("menu/ffb_preset", _selected(preset_ids, preset_list))


func _selected(ids: PackedStringArray, list: OptionButton) -> String:
	return ids[list.selected] if list.selected >= 0 and list.selected < ids.size() else ""


func _describe() -> void:
	var car: Dictionary = sim.describe("cars", _selected(car_ids, car_list))
	var track: Dictionary = sim.describe("tracks", _selected(track_ids, track_list))
	description.text = "%s\n%s" % [car.get("description", ""), track.get("description", "")]


func show_error(message: String) -> void:
	error_label.text = message


func _on_drive() -> void:
	drive_requested.emit(
		_selected(car_ids, car_list),
		_selected(track_ids, track_list),
		auto_clutch.button_pressed,
		auto_shift.button_pressed,
		_selected(preset_ids, preset_list),
		device_ids[device_list.selected],
		direct_steering.button_pressed
	)
