# SPDX-License-Identifier: Apache-2.0
# Player preferences, kept in user://settings.cfg: display, sound, driving
# view, the last menu choices and force-feedback adjustments per preset.
# Nothing here changes how the car behaves; force-feedback values are passed
# to the core, which owns the pipeline.
extends Node

signal changed

const PATH := "user://settings.cfg"
const MSAA_MODES := [Viewport.MSAA_DISABLED, Viewport.MSAA_2X, Viewport.MSAA_4X, Viewport.MSAA_8X]
const DEFAULTS := {
	"display/fullscreen": false,
	"display/vsync": true,
	"display/msaa": 2,
	"display/shadows": true,
	"display/ambient_occlusion": true,
	# D-132: post-process anti-aliasing (0 off, 1 FXAA, 2 SMAA), cheap on
	# video memory beside MSAA, and a lighter ambient occlusion.
	"display/screen_aa": 0,
	"display/ao_light": false,
	"display/fov": 65.0,
	"display/render_scale": 1.0,
	"display/smoke": 2,
	"display/frame_cap": 0,
	"audio/volume": 0.8,
	"driving/ghost": true,
	"driving/lap_telemetry": false,
	"driving/map_mode": 1,
	"driving/seat_view": true,
	"driving/seat_height": 0.0,
	"menu/car": "",
	"menu/track": "",
	"menu/auto_clutch": true,
	"menu/auto_shift": true,
	"menu/traction_control": true,
	"menu/engine_damage": false,
	# 0 none, 1 opponent, 2 tandem lead (D-122).
	"menu/computer_car": 0,
	"menu/ffb_preset": "",
	"ffb/wheel_model": "",
	"ffb/match_rotation": true,
	"input/wizard_done": false,
}

# Graphics presets (D-117): each sets these display values at once; any
# of them can be changed afterwards.
# Low and Medium (D-132) smooth edges with FXAA and SMAA, which work on
# the finished picture, instead of MSAA, whose 2x buffers cost about 214 MB
# of video memory at 1080p (D-128); Medium's ambient occlusion is the
# lightest of Godot's.
const GRAPHICS_PRESETS := {
	"Low": {
		"display/render_scale": 0.7,
		"display/msaa": 0,
		"display/screen_aa": 1,
		"display/shadows": false,
		"display/ambient_occlusion": false,
		"display/ao_light": true,
		"display/smoke": 1,
		"display/frame_cap": 1,
	},
	"Medium": {
		"display/render_scale": 1.0,
		"display/msaa": 0,
		"display/screen_aa": 2,
		"display/shadows": true,
		"display/ambient_occlusion": true,
		"display/ao_light": true,
		"display/smoke": 2,
		"display/frame_cap": 0,
	},
	"High": {
		"display/render_scale": 1.0,
		"display/msaa": 2,
		"display/screen_aa": 0,
		"display/shadows": true,
		"display/ambient_occlusion": true,
		"display/ao_light": false,
		"display/smoke": 2,
		"display/frame_cap": 0,
	},
}
# The frame rate cap (D-134): the monitor's refresh rate, a fixed figure, or
# none (0). It only paces drawing; the physics runs on its own thread at its
# own rate whatever this is.
const FRAME_CAP_NAMES := ["Monitor refresh", "60", "120", "144", "Uncapped"]
const FRAME_CAPS := [-1, 60, 120, 144, 0]


# The cap for Engine.max_fps: the refresh rate for "Monitor refresh" (60 if
# the system will not say), 0 for none.
func frame_cap_fps() -> int:
	var cap: int = FRAME_CAPS[clampi(int(get_value("display/frame_cap")), 0, FRAME_CAPS.size() - 1)]
	if cap < 0:
		var hz := DisplayServer.screen_get_refresh_rate()
		cap = int(round(hz)) if hz > 0.0 else 60
	return cap


const SCREEN_AA_MODES := [Viewport.SCREEN_SPACE_AA_DISABLED, Viewport.SCREEN_SPACE_AA_FXAA, Viewport.SCREEN_SPACE_AA_SMAA]

var values := {}
# Preset id -> {setting name: value}, applied on top of the preset file.
var ffb := {}
# "car__track" -> name of the setup chosen for that pair ("" = as delivered).
var setups := {}


func _init() -> void:
	values = DEFAULTS.duplicate()
	var cfg := ConfigFile.new()
	if cfg.load(PATH) != OK:
		return
	for key in DEFAULTS:
		var parts: PackedStringArray = key.split("/")
		var stored = cfg.get_value(parts[0], parts[1], DEFAULTS[key])
		# A value of the wrong type (hand-edited file) falls back to the default.
		if typeof(stored) == typeof(DEFAULTS[key]):
			values[key] = stored
	if cfg.has_section("setups"):
		for key in cfg.get_section_keys("setups"):
			setups[key] = str(cfg.get_value("setups", key, ""))
	for section in cfg.get_sections():
		if section.begins_with("ffb:"):
			var preset := section.substr(4)
			ffb[preset] = {}
			for name in cfg.get_section_keys(section):
				ffb[preset][name] = float(cfg.get_value(section, name, 0.0))


func save() -> void:
	var cfg := ConfigFile.new()
	for key in values:
		var parts: PackedStringArray = key.split("/")
		cfg.set_value(parts[0], parts[1], values[key])
	for preset in ffb:
		for name in ffb[preset]:
			cfg.set_value("ffb:" + preset, name, ffb[preset][name])
	for key in setups:
		cfg.set_value("setups", key, setups[key])
	cfg.save(PATH)


# The setup chosen for a car and track, "" for the car as delivered.
func get_setup(car: String, track: String) -> String:
	return setups.get(car + "__" + track, "")


func set_setup(car: String, track: String, setup_name: String) -> void:
	setups[car + "__" + track] = setup_name


func get_value(key: String):
	return values.get(key, DEFAULTS.get(key))


func set_value(key: String, value) -> void:
	values[key] = value
	changed.emit()


func apply_graphics_preset(name: String) -> void:
	for key in GRAPHICS_PRESETS.get(name, {}):
		values[key] = GRAPHICS_PRESETS[name][key]
	changed.emit()


func reset_display_and_sound() -> void:
	for key in DEFAULTS:
		if not key.begins_with("menu/"):
			values[key] = DEFAULTS[key]
	changed.emit()


# Remembers one force-feedback adjustment for a preset.
func set_ffb(preset: String, name: String, value: float) -> void:
	if preset == "":
		return
	if not ffb.has(preset):
		ffb[preset] = {}
	ffb[preset][name] = value


# Forgets the adjustments made to a preset.
func clear_ffb(preset: String) -> void:
	ffb.erase(preset)


# Applies the remembered adjustments to the preset the core has loaded.
func apply_ffb(sim) -> void:
	var preset: String = sim.ffb_config().get("id", "")
	for name in ffb.get(preset, {}):
		sim.ffb_set(name, ffb[preset][name])
