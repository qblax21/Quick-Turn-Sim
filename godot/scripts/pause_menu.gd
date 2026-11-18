# SPDX-License-Identifier: Apache-2.0
# Pause menu while driving. The simulation is paused while it is open.
extends CanvasLayer

signal resume_requested
signal restart_requested
signal end_requested
signal quit_requested


func _ready() -> void:
	layer = 10
	var shade := ColorRect.new()
	shade.color = Color(0, 0, 0, 0.45)
	shade.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(shade)
	var center := CenterContainer.new()
	center.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(center)
	var box := VBoxContainer.new()
	box.add_theme_constant_override("separation", 12)
	box.custom_minimum_size = Vector2(360, 0)
	center.add_child(box)
	var title := Label.new()
	title.text = "PAUSED"
	title.add_theme_font_size_override("font_size", 36)
	title.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	box.add_child(title)
	var first: Button
	for entry in [
		["Resume", resume_requested],
		["Restart session", restart_requested],
		["End session and see results", end_requested],
		["Quit to title", quit_requested],
	]:
		var b := Button.new()
		b.text = entry[0]
		b.custom_minimum_size = Vector2(0, 46)
		b.add_theme_font_size_override("font_size", 18)
		var sig: Signal = entry[1]
		b.pressed.connect(func() -> void: sig.emit())
		box.add_child(b)
		if first == null:
			first = b
	var hint := Label.new()
	hint.text = "Esc resumes. Restart puts you back at the start with a fresh session; the session so far is kept as a replay."
	hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	hint.add_theme_font_size_override("font_size", 13)
	box.add_child(hint)
	first.grab_focus.call_deferred()


func _unhandled_key_input(event: InputEvent) -> void:
	if event.pressed and not event.echo and event.physical_keycode == KEY_ESCAPE:
		resume_requested.emit()
		get_viewport().set_input_as_handled()
