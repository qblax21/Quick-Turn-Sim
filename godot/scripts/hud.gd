# SPDX-License-Identifier: Apache-2.0
# Minimal HUD: speed, gear, run time, and a debug overlay with tire
# temperatures, slip, load and the force-feedback signal. Display only.
extends CanvasLayer

const WHEEL_NAMES := ["FL", "FR", "RL", "RR"]
const Minimap := preload("res://scripts/minimap.gd")
const FfbMeter := preload("res://scripts/ffb_meter.gd")

var main_label: Label
var time_label: Label
var debug_label: Label
var notice_label: Label
var delta_label: Label
var limits_label: Label
var invalid_seen := -1
var invalid_shown_until := 0
# Standard atmosphere, bar: gauge pressure is absolute less this.
const ATMOSPHERE_BAR := 1.01325
# Over-rev warning: held a few seconds after the engine comes back down.
var overrev_until := 0
var overrev_peak := 0.0
var flash_text := ""
var flash_until := 0
var input_bars := {}
var ffb_bar: ProgressBar
var debug_visible := false
var surface_names := PackedStringArray()
var surface_tuned := PackedByteArray()
var car_name := ""
var setup_name := ""
var electric := false
var minimap
var ffb_meter
var sim_ref


func setup(sim) -> void:
	sim_ref = sim
	var info: Dictionary = sim.track_info()
	surface_names = info.get("surface_names", PackedStringArray())
	surface_tuned = info.get("surface_tuned", PackedByteArray())
	var car: Dictionary = sim.car_info()
	car_name = car.get("name", "")
	electric = car.get("electric", false)
	setup_name = sim.session_setup_name()
	minimap = Minimap.new()
	minimap.setup(sim)
	minimap.position = Vector2(24.0, 236.0)
	add_child(minimap)


# Map mode: 0 off, 1 whole track, 2 zoomed north up, 3 zoomed heading up.
# A track with nothing to draw never shows a map.
func set_map_mode(mode: int) -> void:
	if minimap:
		minimap.mode = mode
		minimap.visible = mode > 0 and minimap.usable


func update_map(car_xform: Transform3D, ghost_visible: bool, ghost_xform: Transform3D) -> void:
	if minimap and minimap.visible:
		minimap.set_cars(car_xform, ghost_visible, ghost_xform)


func _ready() -> void:
	main_label = Label.new()
	main_label.add_theme_font_size_override("font_size", 34)
	main_label.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_LEFT, Control.PRESET_MODE_MINSIZE, 24)
	main_label.grow_vertical = Control.GROW_DIRECTION_BEGIN
	add_child(main_label)

	time_label = Label.new()
	time_label.add_theme_font_size_override("font_size", 24)
	time_label.set_anchors_and_offsets_preset(Control.PRESET_TOP_LEFT, Control.PRESET_MODE_MINSIZE, 24)
	add_child(time_label)

	notice_label = Label.new()
	notice_label.add_theme_font_size_override("font_size", 14)
	notice_label.add_theme_color_override("font_color", Color(1.0, 0.85, 0.4))
	notice_label.set_anchors_and_offsets_preset(Control.PRESET_CENTER_TOP, Control.PRESET_MODE_MINSIZE, 12)
	notice_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	notice_label.grow_horizontal = Control.GROW_DIRECTION_BOTH
	add_child(notice_label)

	# Time against the best lap, top centre under the notices.
	delta_label = Label.new()
	delta_label.add_theme_font_size_override("font_size", 30)
	delta_label.set_anchors_and_offsets_preset(Control.PRESET_CENTER_TOP, Control.PRESET_MODE_MINSIZE, 12)
	delta_label.position.y = 64.0
	delta_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	delta_label.grow_horizontal = Control.GROW_DIRECTION_BOTH
	add_child(delta_label)

	# Track limits warning, large, under the time difference.
	limits_label = Label.new()
	limits_label.add_theme_font_size_override("font_size", 26)
	limits_label.add_theme_color_override("font_color", Color(1.0, 0.3, 0.25))
	limits_label.add_theme_color_override("font_outline_color", Color(0, 0, 0))
	limits_label.add_theme_constant_override("outline_size", 6)
	limits_label.set_anchors_and_offsets_preset(Control.PRESET_CENTER_TOP, Control.PRESET_MODE_MINSIZE, 12)
	limits_label.position.y = 108.0
	limits_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	limits_label.grow_horizontal = Control.GROW_DIRECTION_BOTH
	add_child(limits_label)

	debug_label = Label.new()
	debug_label.add_theme_font_size_override("font_size", 15)
	debug_label.set_anchors_and_offsets_preset(Control.PRESET_TOP_RIGHT, Control.PRESET_MODE_MINSIZE, 24)
	debug_label.grow_horizontal = Control.GROW_DIRECTION_BEGIN
	debug_label.visible = false
	add_child(debug_label)

	var bars := VBoxContainer.new()
	bars.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_RIGHT, Control.PRESET_MODE_MINSIZE, 24)
	bars.grow_horizontal = Control.GROW_DIRECTION_BEGIN
	bars.grow_vertical = Control.GROW_DIRECTION_BEGIN
	add_child(bars)
	for entry in [["steer", -1.0], ["throttle", 0.0], ["brake", 0.0]]:
		bars.add_child(_bar_row(entry[0], entry[1]))
	var ffb_row := HBoxContainer.new()
	var ffb_caption := Label.new()
	ffb_caption.text = "ffb"
	ffb_caption.custom_minimum_size = Vector2(70, 0)
	ffb_row.add_child(ffb_caption)
	ffb_bar = ProgressBar.new()
	ffb_bar.min_value = -1.0
	ffb_bar.max_value = 1.0
	ffb_bar.step = 0.001
	ffb_bar.show_percentage = false
	ffb_bar.custom_minimum_size = Vector2(220, 14)
	ffb_row.add_child(ffb_bar)
	bars.add_child(ffb_row)

	# Force-feedback tuning meter, bottom centre, off until F4.
	ffb_meter = FfbMeter.new()
	ffb_meter.set_anchors_and_offsets_preset(Control.PRESET_CENTER_BOTTOM, Control.PRESET_MODE_MINSIZE, 24)
	ffb_meter.grow_horizontal = Control.GROW_DIRECTION_BOTH
	ffb_meter.grow_vertical = Control.GROW_DIRECTION_BEGIN
	ffb_meter.position.y -= 20.0
	ffb_meter.visible = false
	add_child(ffb_meter)


func _bar_row(caption: String, minimum: float) -> HBoxContainer:
	var row := HBoxContainer.new()
	var label := Label.new()
	label.text = caption
	label.custom_minimum_size = Vector2(70, 0)
	row.add_child(label)
	var bar := ProgressBar.new()
	bar.min_value = minimum
	bar.max_value = 1.0
	bar.step = 0.001
	bar.show_percentage = false
	bar.custom_minimum_size = Vector2(220, 14)
	row.add_child(bar)
	input_bars[caption] = bar
	return row


# Shows a short message for two seconds.
func flash(text: String) -> void:
	flash_text = text
	flash_until = Time.get_ticks_msec() + 2000


func toggle_ffb_meter() -> void:
	ffb_meter.visible = not ffb_meter.visible


func ffb_meter_visible() -> bool:
	return ffb_meter.visible


func toggle_debug() -> void:
	debug_visible = not debug_visible
	debug_label.visible = debug_visible


func _format_time(seconds: float) -> String:
	if seconds <= 0.0:
		return "--:--.---"
	var minutes := int(seconds / 60.0)
	return "%d:%06.3f" % [minutes, seconds - minutes * 60.0]


func _gear_text(gear: int) -> String:
	if electric:
		return "D" if gear > 0 else ("R" if gear < 0 else "N")
	return "R" if gear < 0 else ("N" if gear == 0 else str(gear))


func update(hud: Dictionary, status: Dictionary, input: Dictionary, telemetry_on: bool, replay_on: bool) -> void:
	if hud.is_empty():
		return
	var speed: float = hud.speed
	var rpm_text := "%.0f rpm" % hud.engine_rpm
	if not electric and hud.get("peak_rpm", 0.0) > 0.0:
		rpm_text += "   peak %.0f" % hud.peak_rpm
	if electric:
		rpm_text = "battery %.0f%%" % (hud.battery_soc * 100.0)
	main_label.text = "%3.0f km/h   %3.0f mph\ngear %s   %s" % [speed * 3.6, speed * 2.23694, _gear_text(hud.gear), rpm_text]

	var run_line := "run   %s" % _format_time(hud.run_time) if hud.run_active else "run   waiting for the start gate"
	time_label.text = "%s\nlast  %s   cones %d\nbest  %s\ncones this run: %d" % [
		run_line, _format_time(hud.last_time), hud.last_cones_hit, _format_time(hud.best_time), hud.cones_hit]
	if hud.get("ghost_time", 0.0) > 0.0:
		time_label.text += "\nghost %s  (setup: %s)" % [_format_time(hud.ghost_time), hud.get("ghost_setup", "")]
	time_label.text += "\nsetup %s" % setup_name

	# Green when ahead of the best lap at this point of the track, red when behind.
	if hud.get("ghost_has_delta", false):
		var delta: float = hud.ghost_delta
		delta_label.text = "%+.2f" % delta
		delta_label.add_theme_color_override("font_color", Color(0.35, 0.95, 0.45) if delta <= 0.0 else Color(1.0, 0.45, 0.4))
	else:
		delta_label.text = ""

	# What the physics used, wherever it was read.
	input_bars["steer"].value = -hud.get("input_steer", input.steer)
	input_bars["throttle"].value = hud.get("input_throttle", input.throttle)
	input_bars["brake"].value = hud.get("input_brake", input.brake)
	ffb_bar.value = status.get("ffb_output", 0.0)
	ffb_bar.modulate = Color(1.0, 0.35, 0.3) if status.get("ffb_clipped", 0.0) > 0.02 else Color(1, 1, 1)

	# Notices: recording state, pause, errors, and anything untuned in use.
	var notices: Array = []
	# A run that missed part of the course: say so for a few seconds.
	var invalid: int = hud.get("invalid_runs", 0)
	if invalid_seen >= 0 and invalid > invalid_seen:
		invalid_shown_until = Time.get_ticks_msec() + 4000
	invalid_seen = invalid
	if Time.get_ticks_msec() < flash_until:
		notices.append(flash_text)
	limits_label.text = "TRACK LIMITS: this lap will not count" if hud.get("limits_broken", false) else ""
	if Time.get_ticks_msec() < invalid_shown_until and not hud.get("limits_broken", false):
		notices.append("LAP NOT COUNTED: off the track or part of the course was missed")
	var damage: float = hud.get("engine_damage", 0.0)
	if hud.get("overrev", false):
		overrev_until = Time.get_ticks_msec() + 4000
		overrev_peak = maxf(overrev_peak, hud.engine_rpm)
	if Time.get_ticks_msec() < overrev_until:
		notices.append("OVER-REV: %.0f rpm, past the limiter (missed downshift)" % overrev_peak)
	else:
		overrev_peak = 0.0
	if damage >= 1.0:
		notices.append("ENGINE FAILED from over-revving: reset the car (R) to repair")
	elif damage > 0.0:
		notices.append("engine damage %.0f%%: power down %.0f%%" % [damage * 100.0, damage * 60.0])
	if hud.get("stalled", false) and damage < 1.0:
		notices.append("ENGINE STALLED: clutch down and the starter (I) to restart")
	if hud.get("grinding", false):
		notices.append("GRINDING: clutch down or match the revs to engage")
	if status.get("paused", false):
		notices.append("PAUSED")
	if telemetry_on:
		notices.append("telemetry recording (%d rows)" % status.get("telemetry_rows", 0))
	if replay_on:
		notices.append("replay recording (%d ticks)" % status.get("replay_frames", 0))
	if not ffb_meter.visible:
		match status.get("ffb_log_state", "off"):
			"waiting":
				notices.append("FFB log: waiting for the start line")
			"recording":
				notices.append("FFB log: recording this lap")
	if ffb_meter.visible and sim_ref:
		ffb_meter.update_from(status, sim_ref.ffb_config())
	var error: String = status.get("error", "")
	if error != "":
		notices.append(error)
	var untuned := {}
	for index in hud.surface:
		if index < surface_tuned.size() and surface_tuned[index] == 0:
			untuned[surface_names[index]] = true
	for name in untuned.keys():
		notices.append("surface model not tuned: %s" % name)
	notice_label.text = "\n".join(notices)

	if debug_visible:
		var lines: Array = [car_name, "wheel   slip    angle    load   surf   core  carc   bar cold/hot   wear  brake °C  pad   surface"]
		for i in range(4):
			var surface_index: int = hud.surface[i]
			var surface := surface_names[surface_index] if surface_index < surface_names.size() else "?"
			var cold_bar: float = hud.get("pressure_cold", hud.pressure)[i] / 100000.0 - ATMOSPHERE_BAR
			lines.append("%s    %+5.2f  %+5.1f°  %5.0f N  %4.0f  %4.0f  %4.0f  %4.2f/%4.2f  %4.1f%%  %5.0f  %4.2f  %s" % [
				WHEEL_NAMES[i], hud.slip_ratio[i], rad_to_deg(hud.slip_angle[i]), hud.load[i],
				hud.temp_surface[i], hud.temp_core[i], hud.temp_carcass[i],
				cold_bar, hud.pressure[i] / 100000.0 - ATMOSPHERE_BAR, hud.get("tire_wear", [0, 0, 0, 0])[i] * 100.0,
				hud.get("brake_temp", [0, 0, 0, 0])[i], hud.get("brake_friction", [1, 1, 1, 1])[i], surface])
		lines.append("")
		lines.append("accel  long %+5.2f g   lat %+5.2f g" % [hud.accel_long / 9.80665, hud.accel_lat / 9.80665])
		lines.append("steering torque %+6.1f N·m" % hud.steering_torque_nm)
		lines.append("ffb  raw %+6.1f  filtered %+6.1f N·m  requested %+5.2f  out %+5.2f" % [
			status.get("ffb_raw_nm", 0.0), status.get("ffb_filtered_nm", 0.0),
			status.get("ffb_requested", 0.0), status.get("ffb_output", 0.0)])
		lines.append("ffb clipping %3.0f%% of the last second" % (status.get("clip_fraction", 0.0) * 100.0))
		var device := "none"
		if status.get("device_open", false):
			device = status.get("device_name", "?")
			if not status.get("device_force_feedback", false):
				device += " (no force feedback)"
		lines.append("wheel: %s" % device)
		var lag := "not measured (needs the wheel mapped both ways and moving)"
		if status.get("steer_lag_measured", false):
			lag = "%.1f ms" % status.get("steer_lag_ms", 0.0)
		lines.append("input  %s   frame hand-over %.1f ms (max %.1f)   device read %.2f ms" % [
			"read on physics thread" if status.get("direct_inputs", false) else "per frame",
			status.get("input_delay_frame_ms", 0.0), status.get("input_delay_frame_max_ms", 0.0),
			status.get("input_delay_direct_ms", 0.0)])
		lines.append("per-frame steering behind the device: %s" % lag)
		lines.append("physics %4.0f Hz   worst wake %d ticks   dropped %.2f s" % [
			status.get("tick_rate", 0.0), status.get("worst_wake_ticks", 0), status.get("dropped_time", 0.0)])
		debug_label.text = "\n".join(lines)
