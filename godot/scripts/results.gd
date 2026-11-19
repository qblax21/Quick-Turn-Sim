# SPDX-License-Identifier: Apache-2.0
# End-of-session results: every counted lap or run with its time, the best
# one, laps that did not count, and the way to the replay. Display only;
# the times are the ones the core measured.
extends CanvasLayer

signal replay_requested(path)
signal again_requested
signal title_requested

# Filled in by the caller: car, track (display names), duration (s),
# laps (Array of {time, cones}), invalid (int), ghost_time (s, 0 if none),
# replay (path or "").
var summary := {}


func setup(data: Dictionary) -> void:
	summary = data


func _format_time(seconds: float) -> String:
	if seconds <= 0.0:
		return "--:--.---"
	var minutes := int(seconds / 60.0)
	return "%d:%06.3f" % [minutes, seconds - minutes * 60.0]


func _ready() -> void:
	var shade := ColorRect.new()
	shade.color = Color(0.03, 0.05, 0.08, 0.5)
	shade.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(shade)
	var center := CenterContainer.new()
	center.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(center)
	var panel := PanelContainer.new()
	panel.custom_minimum_size = Vector2(620, 0)
	center.add_child(panel)
	var margin := MarginContainer.new()
	for side in ["left", "right", "top", "bottom"]:
		margin.add_theme_constant_override("margin_" + side, 28)
	panel.add_child(margin)
	var box := VBoxContainer.new()
	box.add_theme_constant_override("separation", 10)
	margin.add_child(box)

	var title := Label.new()
	title.text = "SESSION RESULTS"
	title.add_theme_font_size_override("font_size", 30)
	box.add_child(title)
	var what := Label.new()
	what.text = "%s at %s, %s on track\nSetup: %s" % [summary.get("car", ""), summary.get("track", ""), _format_time(summary.get("duration", 0.0)), summary.get("setup", "default")]
	what.add_theme_font_size_override("font_size", 17)
	box.add_child(what)
	box.add_child(HSeparator.new())

	var laps: Array = summary.get("laps", [])
	var best := -1
	for i in range(laps.size()):
		if best < 0 or laps[i].time < laps[best].time:
			best = i
	if laps.is_empty():
		var none := Label.new()
		none.text = "No timed lap or run was completed."
		box.add_child(none)
	else:
		var grid := GridContainer.new()
		grid.columns = 3
		grid.add_theme_constant_override("h_separation", 36)
		box.add_child(grid)
		for header in ["Lap", "Time", "Cones"]:
			var h := Label.new()
			h.text = header
			h.add_theme_color_override("font_color", Color(0.7, 0.75, 0.8))
			grid.add_child(h)
		for i in range(laps.size()):
			var mark := "  best" if i == best else ""
			for text in ["%d" % (i + 1), _format_time(laps[i].time) + mark, "%d" % laps[i].cones]:
				var cell := Label.new()
				cell.text = text
				cell.add_theme_font_size_override("font_size", 18)
				if i == best:
					cell.add_theme_color_override("font_color", Color(0.45, 0.95, 0.5))
				grid.add_child(cell)
	box.add_child(HSeparator.new())
	var facts := []
	if best >= 0:
		facts.append("Best this session: %s" % _format_time(laps[best].time))
	var ghost_time: float = summary.get("ghost_time", 0.0)
	if ghost_time > 0.0:
		facts.append("Best ever for this car and track: %s (setup: %s)" % [_format_time(ghost_time), summary.get("ghost_setup", "default")])
	var invalid: int = summary.get("invalid", 0)
	if invalid > 0:
		facts.append("%d lap%s not counted: part of the course was missed." % [invalid, "" if invalid == 1 else "s"])
	if not facts.is_empty():
		var facts_label := Label.new()
		facts_label.text = "\n".join(facts)
		box.add_child(facts_label)

	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 12)
	box.add_child(row)
	var replay_path: String = summary.get("replay", "")
	var watch := _button("Watch replay")
	watch.disabled = replay_path == ""
	watch.pressed.connect(func() -> void: replay_requested.emit(replay_path))
	row.add_child(watch)
	var again := _button("Drive again")
	again.pressed.connect(func() -> void: again_requested.emit())
	row.add_child(again)
	var back := _button("Title")
	back.pressed.connect(func() -> void: title_requested.emit())
	row.add_child(back)
	again.grab_focus.call_deferred()


func _button(text: String) -> Button:
	var b := Button.new()
	b.text = text
	b.custom_minimum_size = Vector2(170, 44)
	return b
