# SPDX-License-Identifier: Apache-2.0
# Force-feedback tuning meter: what the wheel is asked for against what it
# can give, live. Display only; every number comes from the core.
#
#   bar      output sent to the wheel (green, amber above the soft-clip
#            knee, red while clipping)
#   white    force requested before the soft clip, which may run past the
#            end of the scale
#   orange   largest request over the last two seconds
#   lines    soft-clip knee (dashed) and full scale (solid)
extends Control

# The bar spans this many times the wheel's peak torque each way, so an
# overdriven request is still visible.
const SCALE := 1.6
const CLIP_THRESHOLD := 0.02

var output := 0.0
var requested := 0.0
var peak := 0.0
var knee := 0.75
var clipping := false
var text_label: Label


func _ready() -> void:
	custom_minimum_size = Vector2(520, 96)
	text_label = Label.new()
	text_label.add_theme_font_size_override("font_size", 14)
	text_label.position = Vector2(0, 40)
	add_child(text_label)


func update_from(status: Dictionary, config: Dictionary) -> void:
	output = status.get("ffb_output", 0.0)
	requested = status.get("ffb_requested", 0.0)
	peak = status.get("peak_requested", 0.0)
	knee = config.get("soft_clip_knee", 0.75)
	clipping = status.get("ffb_clipped", 0.0) > CLIP_THRESHOLD
	var lines: Array = []
	lines.append("FFB  out %+5.2f   asked %+5.2f   peak %4.2f   clipping %3.0f%% of the last second" % [
		output, requested, peak, status.get("clip_fraction", 0.0) * 100.0])
	lines.append("gain %.2f  [ ]     min force %.2f  ; '     knee %.2f     wheel %.1f N·m" % [
		config.get("gain", 0.0), config.get("min_force", 0.0), knee, config.get("device_max_torque_nm", 0.0)])
	var log_line := "F5 logs the next full lap"
	match status.get("ffb_log_state", "off"):
		"waiting":
			log_line = "FFB log: waiting for the start line (F5 cancels)"
		"recording":
			log_line = "FFB log: recording this lap, %.0f s (F5 stops and saves)" % status.get("ffb_log_seconds", 0.0)
		"saving":
			log_line = "FFB log: writing the file"
		"saved":
			log_line = "FFB log saved%s: %s" % [
				"" if status.get("ffb_log_lap_counted", false) else " (lap not counted or cut short)",
				status.get("ffb_log_path", "")]
		"failed":
			log_line = "FFB log failed: %s" % status.get("ffb_log_path", "")
	lines.append(log_line)
	text_label.text = "\n".join(lines)
	queue_redraw()


func _x(value: float, width: float) -> float:
	var v: float = clamp(value / SCALE, -1.0, 1.0)
	return width * 0.5 * (1.0 + v)


func _draw() -> void:
	var w := size.x
	var h := 30.0
	draw_rect(Rect2(0, 0, w, h), Color(0, 0, 0, 0.55))
	var mid := _x(0.0, w)
	var colour := Color(0.35, 0.85, 0.4)
	if clipping:
		colour = Color(1.0, 0.3, 0.25)
	elif absf(output) > knee:
		colour = Color(1.0, 0.75, 0.25)
	var end := _x(output, w)
	draw_rect(Rect2(min(mid, end), 6, absf(end - mid), h - 12), colour)
	for side in [-1.0, 1.0]:
		var full := _x(side, w)
		draw_line(Vector2(full, 0), Vector2(full, h), Color(1, 1, 1, 0.9), 2.0)
		draw_dashed_line(Vector2(_x(side * knee, w), 0), Vector2(_x(side * knee, w), h), Color(1, 1, 1, 0.6), 1.0, 4.0)
		if peak > 0.0:
			var p := _x(side * peak, w)
			draw_line(Vector2(p, 0), Vector2(p, h), Color(1.0, 0.6, 0.15), 2.0)
	draw_line(Vector2(mid, 0), Vector2(mid, h), Color(1, 1, 1, 0.35), 1.0)
	var r := _x(requested, w)
	draw_line(Vector2(r, -3), Vector2(r, h + 3), Color(1, 1, 1), 2.0)
