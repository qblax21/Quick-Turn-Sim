# SPDX-License-Identifier: Apache-2.0
# Lists saved replays, newest first, and opens the one chosen. Sessions are
# recorded automatically into the replays folder; F3 while driving saves a
# copy of the session so far into the recordings folder.
extends CanvasLayer

signal watch_requested(path)
signal closed

const FOLDERS := ["user://replays", "user://recordings"]

var list: ItemList
var paths: Array = []
var error_label: Label


func _ready() -> void:
	var center := CenterContainer.new()
	center.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(center)
	var panel := PanelContainer.new()
	panel.custom_minimum_size = Vector2(720, 460)
	center.add_child(panel)
	var margin := MarginContainer.new()
	for side in ["left", "right", "top", "bottom"]:
		margin.add_theme_constant_override("margin_" + side, 24)
	panel.add_child(margin)
	var box := VBoxContainer.new()
	box.add_theme_constant_override("separation", 10)
	margin.add_child(box)
	var title := Label.new()
	title.text = "Replays"
	title.add_theme_font_size_override("font_size", 26)
	box.add_child(title)
	list = ItemList.new()
	list.size_flags_vertical = Control.SIZE_EXPAND_FILL
	list.item_activated.connect(func(index: int) -> void: watch_requested.emit(paths[index]))
	box.add_child(list)
	error_label = Label.new()
	error_label.add_theme_color_override("font_color", Color(1.0, 0.5, 0.45))
	error_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	box.add_child(error_label)
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 10)
	box.add_child(row)
	var watch := Button.new()
	watch.text = "Watch"
	watch.custom_minimum_size = Vector2(140, 40)
	watch.pressed.connect(func() -> void:
		var chosen := list.get_selected_items()
		if not chosen.is_empty():
			watch_requested.emit(paths[chosen[0]]))
	row.add_child(watch)
	var back := Button.new()
	back.text = "Back"
	back.custom_minimum_size = Vector2(140, 40)
	back.pressed.connect(func() -> void: closed.emit())
	row.add_child(back)
	_fill()


func _fill() -> void:
	var found: Array = []
	for folder in FOLDERS:
		var dir := DirAccess.open(folder)
		if dir == null:
			continue
		for name in dir.get_files():
			if name.get_extension() == "rpl":
				var path: String = folder.path_join(name)
				found.append([FileAccess.get_modified_time(path), path, name])
	found.sort_custom(func(a: Array, b: Array) -> bool: return a[0] > b[0])
	for entry in found:
		paths.append(ProjectSettings.globalize_path(entry[1]))
		list.add_item(entry[2].get_basename().replace("_", "  "))
	if found.is_empty():
		error_label.text = "No replays yet. Every session is recorded; leave a session and it appears here."
	else:
		list.select(0)


func show_error(message: String) -> void:
	error_label.text = message
