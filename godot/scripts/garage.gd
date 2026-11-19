# SPDX-License-Identifier: Apache-2.0
# Garage: adjust the car's setup for one track, save it under a name and
# pick it for the next drive. The ranges, the defaults and the check that a
# setup is valid all come from the core; this screen only edits numbers.
extends CanvasLayer

signal closed(setup_name)

var sim
var settings
var car_id := ""
var track_id := ""
var items: Array = []
var values := {}          # key -> value, only keys moved off the car's value
var sliders := {}         # key -> HSlider
var readouts := {}        # key -> Label
var name_edit: LineEdit
var saved_list: OptionButton
var saved_names: PackedStringArray
var status: Label


func setup(bridge, store, car: String, track: String) -> void:
	sim = bridge
	settings = store
	car_id = car
	track_id = track


func _ready() -> void:
	layer = 20
	items = sim.setup_items(car_id)
	var shade := ColorRect.new()
	shade.color = Color(0.03, 0.05, 0.08, 1.0)
	shade.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(shade)
	var margin := MarginContainer.new()
	margin.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	for side in ["left", "right", "top", "bottom"]:
		margin.add_theme_constant_override("margin_" + side, 28)
	add_child(margin)
	var outer := VBoxContainer.new()
	outer.add_theme_constant_override("separation", 10)
	margin.add_child(outer)

	var title := Label.new()
	title.text = "Garage: %s at %s" % [sim.describe("cars", car_id).get("name", car_id), sim.describe("tracks", track_id).get("name", track_id)]
	title.add_theme_font_size_override("font_size", 28)
	outer.add_child(title)
	var note := Label.new()
	note.text = "Ranges are what a club car of this class can be set to. Every value starts from the car's data file, which is itself a placeholder (see STATUS.md). The setup is saved with each replay and best lap."
	note.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	note.add_theme_font_size_override("font_size", 13)
	outer.add_child(note)

	# Saved setups and the name to save under.
	var top := HBoxContainer.new()
	top.add_theme_constant_override("separation", 10)
	outer.add_child(top)
	var load_label := Label.new()
	load_label.text = "Saved:"
	top.add_child(load_label)
	saved_list = OptionButton.new()
	saved_list.custom_minimum_size = Vector2(240, 0)
	saved_list.item_selected.connect(_on_pick)
	top.add_child(saved_list)
	var name_label := Label.new()
	name_label.text = "Name:"
	top.add_child(name_label)
	name_edit = LineEdit.new()
	name_edit.custom_minimum_size = Vector2(220, 0)
	name_edit.placeholder_text = "e.g. Ridgeline dry"
	name_edit.max_length = 60
	top.add_child(name_edit)
	for entry in [["Save", _on_save], ["Delete", _on_delete], ["All to car values", _on_reset]]:
		var b := Button.new()
		b.text = entry[0]
		b.pressed.connect(entry[1])
		top.add_child(b)

	# The adjustments, grouped, in two columns.
	var scroll := ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	outer.add_child(scroll)
	var columns := HBoxContainer.new()
	columns.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	columns.add_theme_constant_override("separation", 40)
	scroll.add_child(columns)
	var left := VBoxContainer.new()
	left.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	columns.add_child(left)
	var right := VBoxContainer.new()
	right.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	columns.add_child(right)
	var group := ""
	var column := left
	for item in items:
		if item.group != group:
			group = item.group
			# Gearing and the differential go in the right column.
			if group in ["Brakes", "Differential", "Gearing", "Anti-roll bars"]:
				column = right
			var heading := Label.new()
			heading.text = group
			heading.add_theme_font_size_override("font_size", 20)
			column.add_child(heading)
		_item_row(column, item)

	var bottom := HBoxContainer.new()
	bottom.add_theme_constant_override("separation", 12)
	outer.add_child(bottom)
	var done := Button.new()
	done.text = "Use this setup and return"
	done.custom_minimum_size = Vector2(260, 44)
	done.pressed.connect(_on_done)
	bottom.add_child(done)
	status = Label.new()
	status.add_theme_color_override("font_color", Color(1.0, 0.85, 0.4))
	status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	status.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	bottom.add_child(status)

	_refresh_saved(settings.get_setup(car_id, track_id))
	_on_pick(saved_list.selected)
	done.grab_focus.call_deferred()


func _item_row(parent: Control, item: Dictionary) -> void:
	var row := HBoxContainer.new()
	var label := Label.new()
	label.text = item.label
	label.custom_minimum_size = Vector2(250, 0)
	row.add_child(label)
	var slider := HSlider.new()
	slider.min_value = item.min
	slider.max_value = item.max
	slider.step = item.step
	slider.value = item.default
	slider.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	slider.custom_minimum_size = Vector2(180, 0)
	row.add_child(slider)
	var readout := Label.new()
	readout.custom_minimum_size = Vector2(190, 0)
	row.add_child(readout)
	var key: String = item.key
	slider.value_changed.connect(func(v: float) -> void:
		if absf(v - item.default) < 0.5 * item.step:
			values.erase(key)
		else:
			values[key] = v
		_show(key))
	parent.add_child(row)
	sliders[key] = slider
	readouts[key] = readout
	_show(key)


func _item(key: String) -> Dictionary:
	for item in items:
		if item.key == key:
			return item
	return {}


func _format(item: Dictionary, v: float) -> String:
	var decimals := 0 if item.step >= 1.0 else (1 if item.step >= 0.1 else 2)
	var text := ("%." + str(decimals) + "f %s") % [v, item.unit]
	if item.unit == "kPa":
		text += " (%.1f psi)" % (v * 0.145038)
	return text


func _show(key: String) -> void:
	var item := _item(key)
	var v: float = sliders[key].value if sliders.has(key) else item.default
	var text := _format(item, v)
	if values.has(key):
		text += "   car: " + _format(item, item.default).split(" (")[0]
	readouts[key].text = text
	readouts[key].add_theme_color_override("font_color", Color(1.0, 0.8, 0.35) if values.has(key) else Color(0.85, 0.88, 0.9))


func _refresh_saved(select_name: String) -> void:
	saved_names = sim.setup_list(car_id, track_id)
	saved_list.clear()
	saved_list.add_item("Car as delivered")
	for n in saved_names:
		saved_list.add_item(n)
	var index := saved_names.find(select_name)
	saved_list.select(index + 1 if index >= 0 else 0)


func _apply_values(new_values: Dictionary) -> void:
	values = {}
	for item in items:
		var v: float = new_values.get(item.key, item.default)
		sliders[item.key].set_value_no_signal(v)
		if new_values.has(item.key):
			values[item.key] = v
		_show(item.key)


func _on_pick(index: int) -> void:
	if index <= 0:
		name_edit.text = ""
		_apply_values({})
		return
	var loaded: Dictionary = sim.setup_load(car_id, track_id, saved_names[index - 1])
	if loaded.is_empty():
		status.text = sim.last_error()
		return
	name_edit.text = loaded.name
	_apply_values(loaded.values)
	status.text = ""


func _on_save() -> void:
	var setup_name := name_edit.text.strip_edges()
	if setup_name == "":
		status.text = "Give the setup a name first."
		return
	if sim.setup_save(car_id, track_id, setup_name, values):
		_refresh_saved(setup_name)
		status.text = "Saved \"%s\"." % setup_name
	else:
		status.text = sim.last_error()


func _on_delete() -> void:
	var index := saved_list.selected
	if index <= 0:
		return
	sim.setup_delete(car_id, track_id, saved_names[index - 1])
	_refresh_saved("")
	_on_pick(0)
	status.text = "Deleted."


func _on_reset() -> void:
	_apply_values({})
	status.text = "Every value is back to the car's own."


func _same(a: Dictionary, b: Dictionary) -> bool:
	if a.size() != b.size():
		return false
	for key in a:
		if not b.has(key) or absf(float(a[key]) - float(b[key])) > 1e-6:
			return false
	return true


# Uses what is on screen. A setup with changes but no saved name is used
# for the next drive as "unnamed" and not kept.
func _on_done() -> void:
	var setup_name := name_edit.text.strip_edges()
	# Named only if it is exactly what is saved under that name.
	var saved: bool = setup_name in saved_names and _same(sim.setup_load(car_id, track_id, setup_name).get("values", {}), values)
	if not sim.setup_use(car_id, setup_name if saved else "", values):
		status.text = sim.last_error()
		return
	settings.set_setup(car_id, track_id, setup_name if saved else "")
	settings.save()
	closed.emit(setup_name if saved else ("" if values.is_empty() else "unnamed"))
