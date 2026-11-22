# SPDX-License-Identifier: Apache-2.0
# Live delta bar against the best lap (the ghost): green to the left when
# ahead, red to the right when behind, full at one second either way, with
# a marker for which way it is moving over the last half second. Display
# only; the delta comes from the core (GhostRecorder::delta_s).
extends Control

const FULL_SCALE_S := 1.0
const TREND_WINDOW_S := 0.5

var delta := 0.0
var has_delta := false
var history: Array = []  # [time_s, delta] pairs
var trend := 0.0


func _ready() -> void:
	custom_minimum_size = Vector2(360, 16)
	mouse_filter = Control.MOUSE_FILTER_IGNORE


func set_delta(value: float, valid: bool) -> void:
	has_delta = valid
	delta = value
	var now := Time.get_ticks_msec() / 1000.0
	if valid:
		history.append([now, value])
	else:
		history.clear()
	while history.size() > 1 and now - history[0][0] > TREND_WINDOW_S:
		history.pop_front()
	trend = 0.0
	if history.size() > 1:
		var span: float = history[-1][0] - history[0][0]
		if span > 0.1:
			trend = (history[-1][1] - history[0][1]) / span
	queue_redraw()


func _draw() -> void:
	if not has_delta:
		return
	var w := size.x
	var h := size.y
	var mid := w * 0.5
	draw_rect(Rect2(0, 0, w, h), Color(0, 0, 0, 0.45))
	var f: float = clampf(delta / FULL_SCALE_S, -1.0, 1.0)
	var end := mid + f * mid
	var colour := Color(0.3, 0.9, 0.4) if delta <= 0.0 else Color(1.0, 0.4, 0.35)
	draw_rect(Rect2(minf(mid, end), 2, absf(end - mid), h - 4), colour)
	draw_line(Vector2(mid, -2), Vector2(mid, h + 2), Color(1, 1, 1, 0.9), 2.0)
	# Which way it is going: a small triangle at the bar's end, pointing
	# right while losing time and left while gaining.
	if absf(trend) > 0.02:
		var dir := 1.0 if trend > 0.0 else -1.0
		var x := clampf(end + dir * 8.0, 6.0, w - 6.0)
		var tip := Vector2(x + dir * 6.0, h * 0.5)
		draw_colored_polygon(PackedVector2Array([tip, Vector2(x, 1), Vector2(x, h - 1)]),
			Color(1.0, 0.45, 0.4) if dir > 0.0 else Color(0.35, 0.95, 0.45))
