# SPDX-License-Identifier: Apache-2.0
# Settings screen: display, sound and driving view. Changes apply at once
# and are saved when the screen closes. Controls and force feedback have
# their own screen.
extends CanvasLayer

signal closed

var settings


func setup(store) -> void:
	settings = store


func _ready() -> void:
	var center := CenterContainer.new()
	center.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(center)
	var panel := PanelContainer.new()
	panel.custom_minimum_size = Vector2(640, 0)
	center.add_child(panel)
	var margin := MarginContainer.new()
	for side in ["left", "right", "top", "bottom"]:
		margin.add_theme_constant_override("margin_" + side, 24)
	panel.add_child(margin)
	var box := VBoxContainer.new()
	box.add_theme_constant_override("separation", 8)
	margin.add_child(box)

	_heading(box, "Display")
	_check(box, "Fullscreen", "display/fullscreen")
	_check(box, "Vertical sync", "display/vsync")
	_choice(box, "Anti-aliasing", "display/msaa", ["Off", "2x", "4x", "8x"])
	_check(box, "Shadows", "display/shadows")
	_check(box, "Ambient occlusion", "display/ambient_occlusion")
	_slider(box, "Field of view (degrees)", "display/fov", 40.0, 100.0, 1.0)

	_heading(box, "Sound")
	_slider(box, "Volume", "audio/volume", 0.0, 1.0, 0.01)

	_heading(box, "Driving")
	_check(box, "Start in the driver's seat (C switches while driving)", "driving/seat_view")
	_slider(box, "Seat height adjustment (m)", "driving/seat_height", -0.15, 0.15, 0.01)
	_check(box, "Show the ghost of the best lap (G switches while driving)", "driving/ghost")
	_choice(box, "Track map (N switches while driving)", "driving/map_mode", ["Off", "Whole track, north up", "Zoomed, north up", "Zoomed, heading up"])

	box.add_child(HSeparator.new())
	var buttons := HBoxContainer.new()
	buttons.add_theme_constant_override("separation", 12)
	box.add_child(buttons)
	var done := Button.new()
	done.text = "Save and return"
	done.custom_minimum_size = Vector2(200, 44)
	done.pressed.connect(func() -> void:
		settings.save()
		closed.emit())
	buttons.add_child(done)
	var defaults := Button.new()
	defaults.text = "Restore defaults"
	defaults.custom_minimum_size = Vector2(0, 44)
	defaults.pressed.connect(func() -> void:
		settings.reset_display_and_sound()
		settings.save()
		closed.emit())
	buttons.add_child(defaults)


func _heading(parent: Control, text: String) -> void:
	var label := Label.new()
	label.text = text
	label.add_theme_font_size_override("font_size", 22)
	parent.add_child(label)


func _row(parent: Control, caption: String, control: Control) -> void:
	var row := HBoxContainer.new()
	var label := Label.new()
	label.text = caption
	label.custom_minimum_size = Vector2(260, 0)
	row.add_child(label)
	control.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	row.add_child(control)
	parent.add_child(row)


func _check(parent: Control, caption: String, key: String) -> void:
	var check := CheckBox.new()
	check.text = caption
	check.button_pressed = settings.get_value(key)
	check.toggled.connect(func(on: bool) -> void: settings.set_value(key, on))
	parent.add_child(check)


func _choice(parent: Control, caption: String, key: String, names: Array) -> void:
	var list := OptionButton.new()
	for item in names:
		list.add_item(item)
	list.select(clampi(settings.get_value(key), 0, names.size() - 1))
	list.item_selected.connect(func(index: int) -> void: settings.set_value(key, index))
	_row(parent, caption, list)


func _slider(parent: Control, caption: String, key: String, low: float, high: float, step: float) -> void:
	var row := HBoxContainer.new()
	var slider := HSlider.new()
	slider.min_value = low
	slider.max_value = high
	slider.step = step
	slider.value = settings.get_value(key)
	slider.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	slider.custom_minimum_size = Vector2(220, 0)
	row.add_child(slider)
	var readout := Label.new()
	readout.custom_minimum_size = Vector2(60, 0)
	readout.text = str(snappedf(slider.value, step))
	row.add_child(readout)
	slider.value_changed.connect(func(v: float) -> void:
		readout.text = str(snappedf(v, step))
		settings.set_value(key, v))
	_row(parent, caption, row)
