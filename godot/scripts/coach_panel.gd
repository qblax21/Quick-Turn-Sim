# SPDX-License-Identifier: Apache-2.0
# After-lap coach (F7, and for a while after each counted lap): the lap
# split into corners, the time lost or gained in each against the best lap
# before it, and why. Display only; the analysis is in the core
# (qts-sim/src/coach.rs) and reads the lap's telemetry.
extends PanelContainer

const SHOW_AFTER_LAP_S := 12.0
const HIGHLIGHT := 3

var serial := -1
var hide_at := 0.0
var pinned := false
var box: VBoxContainer


func _ready() -> void:
	mouse_filter = Control.MOUSE_FILTER_IGNORE
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0, 0, 0, 0.7)
	style.set_content_margin_all(12)
	add_theme_stylebox_override("panel", style)
	box = VBoxContainer.new()
	add_child(box)
	visible = false


func _format(seconds: float) -> String:
	var m := int(seconds / 60.0)
	return "%d:%06.3f" % [m, seconds - 60.0 * m]


func _label(text: String, size: int, colour := Color(1, 1, 1)) -> Label:
	var l := Label.new()
	l.text = text
	l.add_theme_font_size_override("font_size", size)
	l.add_theme_color_override("font_color", colour)
	box.add_child(l)
	return l


# Shows the panel until toggled off, or hides it.
func toggle() -> void:
	pinned = not visible
	visible = pinned


func update_from(coach: Dictionary) -> void:
	var s: int = coach.get("serial", 0)
	if s != serial and coach.get("has_report", false):
		serial = s
		_build(coach)
		if not visible:
			visible = true
			hide_at = Time.get_ticks_msec() / 1000.0 + SHOW_AFTER_LAP_S
	elif serial < 0:
		_build(coach)
	if visible and not pinned and hide_at > 0.0 and Time.get_ticks_msec() / 1000.0 > hide_at:
		visible = false
		hide_at = 0.0


func _build(coach: Dictionary) -> void:
	for c in box.get_children():
		c.queue_free()
	if not coach.get("has_report", false):
		_label("Coach: drive two counted laps; each lap is then compared", 18)
		_label("corner by corner with your best so far.", 18)
		return
	var lap: float = coach.lap_s
	var ref: float = coach.reference_s
	var diff := lap - ref
	_label("Lap %s against best %s   %+.3f s" % [_format(lap), _format(ref), diff], 20,
		Color(0.35, 0.95, 0.45) if diff <= 0.0 else Color(1.0, 0.5, 0.45))
	var corners: Array = coach.get("corners", [])
	# The corners that cost most, to mark them.
	var losses: Array = []
	for c in corners:
		if c.time_lost > 0.02:
			losses.append(c.time_lost)
	losses.sort()
	losses.reverse()
	var cut: float = losses[min(HIGHLIGHT, losses.size()) - 1] if losses.size() > 0 else INF
	for c in corners:
		var lost: float = c.time_lost
		var text := "T%-2d %-5s %+.2f s   " % [c.number, "left" if c.left else "right", lost]
		var colour := Color(0.75, 0.75, 0.75)
		if lost >= cut:
			text += c.text
			colour = Color(1.0, 0.6, 0.4)
		elif lost > 0.02:
			text += c.text
			colour = Color(1, 1, 1)
		elif lost < -0.02:
			text += "gained"
			colour = Color(0.5, 0.9, 0.55)
		_label(text, 16, colour)
	var error: String = coach.get("error", "")
	if error != "":
		_label(error, 14, Color(1.0, 0.5, 0.4))
