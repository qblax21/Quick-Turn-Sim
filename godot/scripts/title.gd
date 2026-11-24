# SPDX-License-Identifier: Apache-2.0
# Title screen: the name, what this is, and the way in. The generated sky
# of the main scene shows behind it.
extends CanvasLayer

signal drive_requested
signal replays_requested
signal controls_requested
signal settings_requested

var sim


func setup(bridge) -> void:
	sim = bridge


func _ready() -> void:
	var shade := ColorRect.new()
	shade.color = Color(0.03, 0.05, 0.08, 0.35)
	shade.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	shade.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(shade)
	var center := CenterContainer.new()
	center.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(center)
	var box := VBoxContainer.new()
	box.add_theme_constant_override("separation", 14)
	box.custom_minimum_size = Vector2(520, 0)
	center.add_child(box)

	var name_label := Label.new()
	name_label.text = sim.product_name().to_upper()
	name_label.add_theme_font_size_override("font_size", 64)
	name_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	name_label.add_theme_color_override("font_shadow_color", Color(0, 0, 0, 0.5))
	name_label.add_theme_constant_override("shadow_offset_y", 3)
	box.add_child(name_label)
	var tagline := Label.new()
	tagline.text = "A physics-first driving simulator for wheel users"
	tagline.add_theme_font_size_override("font_size", 20)
	tagline.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	box.add_child(tagline)
	box.add_child(_spacer(18))

	var drive := _button("Drive", 56, 24)
	drive.pressed.connect(func() -> void: drive_requested.emit())
	box.add_child(drive)
	for entry in [["Replays", replays_requested], ["Controls and wheel", controls_requested], ["Settings", settings_requested]]:
		var b := _button(entry[0], 44, 18)
		var sig: Signal = entry[1]
		b.pressed.connect(func() -> void: sig.emit())
		box.add_child(b)
	var quit := _button("Quit", 44, 18)
	quit.pressed.connect(func() -> void: get_tree().quit())
	box.add_child(quit)
	drive.grab_focus.call_deferred()

	var footer := Label.new()
	footer.text = "Version %s. Early build: the cars and tracks are fictional, and every vehicle, tire and surface parameter is a placeholder, not a measurement." % sim.version()
	footer.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	footer.add_theme_font_size_override("font_size", 13)
	footer.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_WIDE, Control.PRESET_MODE_MINSIZE, 20)
	footer.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	footer.grow_vertical = Control.GROW_DIRECTION_BEGIN
	add_child(footer)

	# Optional donations (D-120): a quiet link in the corner, nothing else.
	# The game is bought once and every later improvement is free.
	var url: String = sim.donate_url()
	if url != "":
		var support := LinkButton.new()
		support.text = "Support development"
		support.tooltip_text = "Optional. Opens %s in your browser. Donations unlock nothing; every update stays free." % url
		support.add_theme_font_size_override("font_size", 13)
		support.underline = LinkButton.UNDERLINE_MODE_ON_HOVER
		support.pressed.connect(func() -> void: OS.shell_open(url))
		support.set_anchors_and_offsets_preset(Control.PRESET_TOP_RIGHT, Control.PRESET_MODE_MINSIZE, 16)
		support.grow_horizontal = Control.GROW_DIRECTION_BEGIN
		add_child(support)


func _button(text: String, height: float, font_size: int) -> Button:
	var b := Button.new()
	b.text = text
	b.custom_minimum_size = Vector2(0, height)
	b.add_theme_font_size_override("font_size", font_size)
	return b


func _spacer(height: float) -> Control:
	var c := Control.new()
	c.custom_minimum_size = Vector2(0, height)
	return c
