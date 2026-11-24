# SPDX-License-Identifier: Apache-2.0
# Drift judging on screen (D-114), on tracks with judged zones only:
#   readout  live slip angle, the zone being judged, the last corner's score
#            (line, angle, style out of 35, 35, 30) and the lap so far
#   panel    after each lap, and on F9: every corner's score and why, the
#            lap's score and the best, and the corner that cost the most
# Display only; the judging is in the core (qts-sim/src/drift.rs).
extends VBoxContainer

const SHOW_AFTER_LAP_S := 12.0

var readout: Label
var panel: PanelContainer
var rows: VBoxContainer
var serial := -1
var hide_at := 0.0
var pinned := false


func _ready() -> void:
	mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_theme_constant_override("separation", 8)
	readout = Label.new()
	readout.add_theme_font_size_override("font_size", 18)
	add_child(readout)
	panel = PanelContainer.new()
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0, 0, 0, 0.7)
	style.set_content_margin_all(12)
	panel.add_theme_stylebox_override("panel", style)
	rows = VBoxContainer.new()
	panel.add_child(rows)
	panel.visible = false
	add_child(panel)
	visible = false


func toggle_panel() -> void:
	pinned = not panel.visible
	panel.visible = pinned


func _row(text: String, size: int, colour := Color(1, 1, 1)) -> void:
	var l := Label.new()
	l.text = text
	l.add_theme_font_size_override("font_size", size)
	l.add_theme_color_override("font_color", colour)
	rows.add_child(l)


static func _parts(z: Dictionary) -> String:
	return "line %2.0f  angle %2.0f  style %2.0f" % [z.line * 35.0, z.angle * 35.0, z.style * 30.0]


func update_from(d: Dictionary) -> void:
	visible = d.get("active", false)
	if not visible:
		return
	var lines: Array = []
	var zone: String = d.get("zone", "")
	lines.append("DRIFT  %3.0f deg%s" % [d.get("angle", 0.0), ("   judging " + zone) if zone != "" else ""])
	if d.get("has_last_zone", false):
		var z: Dictionary = d.last_zone
		lines.append("%s  %3.0f   %s" % [z.name, z.total, "penalty" if z.penalty else _parts(z)])
	if d.get("zones_so_far", 0) > 0:
		lines.append("lap so far %4.1f  (%d corners)" % [d.lap_so_far, d.zones_so_far])
	readout.text = "\n".join(lines)
	var s: int = d.get("serial", 0)
	if s != serial and d.get("has_lap", false):
		serial = s
		_build(d)
		if not panel.visible:
			panel.visible = true
			hide_at = Time.get_ticks_msec() / 1000.0 + SHOW_AFTER_LAP_S
	if panel.visible and not pinned and hide_at > 0.0 and Time.get_ticks_msec() / 1000.0 > hide_at:
		panel.visible = false
		hide_at = 0.0


func _build(d: Dictionary) -> void:
	for c in rows.get_children():
		c.queue_free()
	var best: float = d.get("best", -1.0)
	_row("Drift lap  %.1f / 100%s%s" % [d.lap_total, "" if d.lap_counted else "   (lap not counted)",
		("   best %.1f" % best) if best >= 0.0 else ""], 20, Color(1.0, 0.85, 0.4))
	# Coach (D-118): the corner that cost the most points, and why.
	var coach: String = d.get("coach", "")
	if coach != "":
		_row(coach, 17, Color(0.55, 0.85, 1.0))
	for z in d.get("zones", []):
		if z.get("missed", false):
			_row("--   not judged", 15, Color(0.6, 0.6, 0.6))
			continue
		var colour := Color(1.0, 0.5, 0.45) if z.penalty else (Color(0.5, 0.95, 0.55) if z.total >= 70.0 else Color(1, 1, 1))
		_row("%-3s %5.1f   %s   %s" % [z.name, z.total, "" if z.penalty else _parts(z), z.text], 15, colour)
