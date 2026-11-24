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
#   CLIP     lamp at the right end: lit red while the soft clip is removing
#            force, held for half a second so a short clip is not missed
#   graph    the last six seconds of output (green) and request (white)
#            against full scale and the knee (D-123)
#   gain     K, or the button, sets the gain the core suggests for clipping
#            under 2 % of this lap (D-123)
# Cheap to draw: two polylines of at most GRAPH_POINTS points, sampled at
# GRAPH_HZ whatever the frame rate.
extends Control

signal gain_requested

const GRAPH_S := 6.0
const GRAPH_HZ := 60.0
const GRAPH_POINTS := int(GRAPH_S * GRAPH_HZ)
const GRAPH_H := 64.0
const BAR_H := 30.0

# The bar spans this many times the wheel's peak torque each way, so an
# overdriven request is still visible.
const SCALE := 1.6
const CLIP_THRESHOLD := 0.02
const CLIP_HOLD_S := 0.5
const LAMP_W := 56.0

var output := 0.0
var requested := 0.0
var peak := 0.0
var knee := 0.75
var clipping := false
var clip_hold := 0.0
var text_label: Label
var gain_button: Button
var hist_out := PackedFloat32Array()
var hist_req := PackedFloat32Array()
var hist_head := 0
var sample_wait := 0.0


func _ready() -> void:
	custom_minimum_size = Vector2(560, 230)
	hist_out.resize(GRAPH_POINTS)
	hist_req.resize(GRAPH_POINTS)
	text_label = Label.new()
	text_label.add_theme_font_size_override("font_size", 14)
	text_label.position = Vector2(0, BAR_H + GRAPH_H + 16)
	add_child(text_label)
	gain_button = Button.new()
	gain_button.text = "Set gain (K)"
	gain_button.focus_mode = Control.FOCUS_NONE
	gain_button.pressed.connect(func() -> void: gain_requested.emit())
	add_child(gain_button)


func update_from(status: Dictionary, config: Dictionary) -> void:
	output = status.get("ffb_output", 0.0)
	requested = status.get("ffb_requested", 0.0)
	peak = status.get("peak_requested", 0.0)
	knee = config.get("soft_clip_knee", 0.75)
	clipping = status.get("ffb_clipped", 0.0) > CLIP_THRESHOLD
	if clipping:
		clip_hold = CLIP_HOLD_S
	var lines: Array = []
	lines.append("FFB  out %+5.2f   asked %+5.2f   peak %4.2f   clipping %3.0f%% of the last second" % [
		output, requested, peak, status.get("clip_fraction", 0.0) * 100.0])
	var last_lap: float = status.get("clip_last_lap_percent", -1.0)
	lines.append("clipped %4.1f%% of this lap   last lap %s" % [
		status.get("clip_lap_percent", 0.0), "--" if last_lap < 0.0 else "%4.1f%%" % last_lap])
	var wheel_nm: float = config.get("device_max_torque_nm", 0.0)
	lines.append("column %+5.1f N·m  ->  wheel asked %+5.1f N·m, given %+5.1f N·m of %.1f N·m   (peak asked %.1f N·m)" % [
		status.get("ffb_raw_nm", 0.0), requested * wheel_nm, output * wheel_nm, wheel_nm, peak * wheel_nm])
	lines.append("gain %.2f (wheel N·m per column N·m)  [ ]     min force %.2f  ; '     knee %.2f" % [
		config.get("gain", 0.0), config.get("min_force", 0.0), knee])
	var suggested: float = status.get("suggested_gain", -1.0)
	gain_button.disabled = suggested <= 0.0
	if suggested > 0.0:
		lines.append("K: gain %.2f keeps clipping under 2%% of this lap (from %.2f)" % [suggested, config.get("gain", 0.0)])
	else:
		lines.append("K: drive 5 s of a timed lap for a gain suggestion")
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


func _process(delta: float) -> void:
	if not visible:
		return
	sample_wait -= delta
	if sample_wait <= 0.0:
		sample_wait += 1.0 / GRAPH_HZ
		if sample_wait < 0.0:
			sample_wait = 0.0
		hist_out[hist_head] = output
		hist_req[hist_head] = requested
		hist_head = (hist_head + 1) % GRAPH_POINTS
		queue_redraw()
	if clip_hold > 0.0:
		clip_hold = maxf(clip_hold - delta, 0.0)
		if clip_hold == 0.0:
			queue_redraw()


func _x(value: float, width: float) -> float:
	var v: float = clamp(value / SCALE, -1.0, 1.0)
	return width * 0.5 * (1.0 + v)


func _draw() -> void:
	var h := 30.0
	# The clip lamp sits to the right of the bar.
	var lamp := Rect2(size.x - LAMP_W, 0, LAMP_W, h)
	var lit := clip_hold > 0.0
	draw_rect(lamp, Color(0.85, 0.12, 0.1) if lit else Color(0.18, 0.05, 0.05, 0.8))
	draw_rect(lamp, Color(1, 1, 1, 0.5), false, 1.0)
	var font := get_theme_default_font()
	var fs := 14
	var tw := font.get_string_size("CLIP", HORIZONTAL_ALIGNMENT_LEFT, -1, fs).x
	draw_string(font, Vector2(lamp.position.x + (LAMP_W - tw) * 0.5, h * 0.5 + fs * 0.35), "CLIP",
		HORIZONTAL_ALIGNMENT_LEFT, -1, fs, Color(1, 1, 1) if lit else Color(1, 1, 1, 0.35))
	var w := size.x - LAMP_W - 8.0
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
	# The graph leaves room on its right for the gain button.
	var gw := size.x - gain_button.size.x - 12.0
	_draw_graph(gw)
	gain_button.position = Vector2(gw + 12.0, BAR_H + 8.0 + (GRAPH_H - gain_button.size.y) * 0.5)


# Six seconds of output and request: time runs left to right, full scale
# each way at the top and bottom edges of the band SCALE allows.
func _draw_graph(w: float) -> void:
	var top := BAR_H + 8.0
	var mid := top + GRAPH_H * 0.5
	var k := GRAPH_H * 0.5 / SCALE
	draw_rect(Rect2(0, top, w, GRAPH_H), Color(0, 0, 0, 0.55))
	for side in [-1.0, 1.0]:
		draw_line(Vector2(0, mid - side * k), Vector2(w, mid - side * k), Color(1, 1, 1, 0.7), 1.0)
		draw_dashed_line(Vector2(0, mid - side * knee * k), Vector2(w, mid - side * knee * k), Color(1, 1, 1, 0.35), 1.0, 4.0)
	var out_pts := PackedVector2Array()
	var req_pts := PackedVector2Array()
	out_pts.resize(GRAPH_POINTS)
	req_pts.resize(GRAPH_POINTS)
	var dx := w / float(GRAPH_POINTS - 1)
	for i in GRAPH_POINTS:
		var j := (hist_head + i) % GRAPH_POINTS
		out_pts[i] = Vector2(i * dx, mid - clampf(hist_out[j], -SCALE, SCALE) * k)
		req_pts[i] = Vector2(i * dx, mid - clampf(hist_req[j], -SCALE, SCALE) * k)
	draw_polyline(req_pts, Color(1, 1, 1, 0.8), 1.0)
	draw_polyline(out_pts, Color(0.35, 0.85, 0.4), 1.5)
