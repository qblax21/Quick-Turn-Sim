# SPDX-License-Identifier: Apache-2.0
# Frame-time and physics-step overlay (F6). Display only: the frame times
# are this renderer's own; every physics figure comes from the host thread,
# which steps the physics and force feedback at a fixed rate apart from
# rendering.
#
#   frame    render frame time now and the worst over the last second, and
#            a strip of the last 120 frames (red above 33 ms)
#   physics  the fixed rate, the rate measured over the last second, the
#            mean and worst time of one tick (physics and force feedback)
#            against the tick budget, late wake-ups and dropped time
extends Control

const HISTORY := 120
const WINDOW_S := 1.0

var frames: PackedFloat32Array = PackedFloat32Array()
var window_worst := 0.0
var window_elapsed := 0.0
var shown_worst := 0.0
var label: Label


func _ready() -> void:
	custom_minimum_size = Vector2(490, 128)
	mouse_filter = Control.MOUSE_FILTER_IGNORE
	label = Label.new()
	label.add_theme_font_size_override("font_size", 14)
	label.position = Vector2(8, 4)
	add_child(label)


func _process(delta: float) -> void:
	if not visible:
		return
	var ms := delta * 1000.0
	frames.append(ms)
	if frames.size() > HISTORY:
		frames.remove_at(0)
	window_worst = maxf(window_worst, ms)
	window_elapsed += delta
	if window_elapsed >= WINDOW_S:
		shown_worst = window_worst
		window_worst = 0.0
		window_elapsed = 0.0
	queue_redraw()


func update_from(status: Dictionary) -> void:
	if not visible:
		return
	var fixed: int = status.get("tick_rate_fixed", 0)
	var budget_us := 1.0e6 / maxf(float(fixed), 1.0)
	var worst_us: float = status.get("step_worst_us", 0.0)
	var now_ms: float = frames[frames.size() - 1] if frames.size() > 0 else 0.0
	var lines: Array = []
	lines.append("frame %5.1f ms  (%3.0f fps)   worst %5.1f ms last second" % [
		now_ms, 1000.0 / maxf(now_ms, 0.001), shown_worst])
	lines.append("physics + FFB  fixed %d Hz, measured %4.0f Hz, apart from rendering" % [
		fixed, status.get("tick_rate", 0.0)])
	lines.append("step  mean %4.0f µs   worst %4.0f µs   of %4.0f µs  (%3.0f%% of budget)" % [
		status.get("step_mean_us", 0.0), worst_us, budget_us, 100.0 * worst_us / budget_us])
	lines.append("late wake-up %d ticks   dropped %.2f s" % [
		status.get("worst_wake_ticks", 0), status.get("dropped_time", 0.0)])
	label.text = "\n".join(lines)


func _draw() -> void:
	draw_rect(Rect2(Vector2.ZERO, size), Color(0, 0, 0, 0.55))
	# Frame-time strip along the bottom, 0 to 50 ms, with a 33 ms line.
	var h := 30.0
	var top := size.y - h - 4.0
	var w := size.x - 16.0
	var bar := w / float(HISTORY)
	for i in frames.size():
		var ms: float = frames[i]
		var bh: float = clampf(ms / 50.0, 0.0, 1.0) * h
		var colour := Color(0.85, 0.25, 0.2) if ms > 33.4 else Color(0.4, 0.85, 0.45)
		draw_rect(Rect2(8.0 + i * bar, top + h - bh, maxf(bar - 1.0, 1.0), bh), colour)
	var y33 := top + h - (33.3 / 50.0) * h
	draw_dashed_line(Vector2(8, y33), Vector2(8 + w, y33), Color(1, 1, 1, 0.5), 1.0, 4.0)
