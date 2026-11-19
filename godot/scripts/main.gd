# SPDX-License-Identifier: Apache-2.0
# Front-end entry point. Owns the bridge to the simulation core and switches
# between the menu and driving. Nothing here computes physics, timing or
# force feedback: it sends inputs and draws what the core reports.
extends Node3D

const Menu := preload("res://scripts/menu.gd")
const InputSetup := preload("res://scripts/input_setup.gd")
const InputReader := preload("res://scripts/input_reader.gd")
const World := preload("res://scripts/world.gd")
const CarView := preload("res://scripts/car_view.gd")
const Hud := preload("res://scripts/hud.gd")
const SkidMarks := preload("res://scripts/skid_marks.gd")
const Audio := preload("res://scripts/audio.gd")
const Settings := preload("res://scripts/settings.gd")
const SettingsScreen := preload("res://scripts/settings_screen.gd")
const ReplayPicker := preload("res://scripts/replay_picker.gd")
const ReplayView := preload("res://scripts/replay_view.gd")
const Title := preload("res://scripts/title.gd")
const PauseMenu := preload("res://scripts/pause_menu.gd")
const Results := preload("res://scripts/results.gd")
# Automatically saved session replays kept; older ones are deleted.
const KEEP_SESSION_REPLAYS := 20
# Sessions shorter than this are not kept, s.
const MIN_SESSION_REPLAY_S := 10.0

# In the seat view the ghost is hidden when it is this close, so it never
# fills the windscreen.
const GHOST_HIDE_DISTANCE := 5.0

var sim  # SimBridge, from the binding library
var settings
var reader
var menu
var setup
var settings_screen
var title
# Scenery behind the title screen: a track with a camera flying round it.
var scenery
var scenery_points := PackedVector3Array()
var scenery_s := 0.0
var pause_menu
var results
# The last drive request, so a session can be restarted or driven again.
var last_drive: Array = []
# Counted laps or runs this session: {time, cones}.
var session_laps: Array = []
var session_runs_seen := 0
var session_started_ms := 0
var session_replay := ""
var replay_picker
var replay_view
var session_car := ""
var session_track := ""
var world
var car_view
var ghost_view
var skid_marks
var hud
var audio
var camera: Camera3D
var sun: DirectionalLight3D
var environment: Environment
# C switches between the driver's seat and the chase camera.
var cockpit_view := true
var show_ghost := true
# Track map: 0 off, 1 whole track, 2 zoomed north up, 3 zoomed heading up.
var map_mode := 1
var paused := false
var telemetry_on := false
var replay_on := false
# Set by the self-test in place of the player's controls.
var fixed_input := {}


func _ready() -> void:
	if not ClassDB.class_exists("SimBridge"):
		_fatal("The simulation library is not loaded.\nBuild it first (see README), then restart.")
		return
	sim = ClassDB.instantiate("SimBridge")
	get_window().title = sim.product_name()
	var data_dir := _find_data_dir()
	if data_dir == "" or not sim.set_data_dir(data_dir):
		_fatal("The data folder was not found next to the application or the project.")
		return
	sim.set_ghost_dir(ProjectSettings.globalize_path("user://ghosts"))
	sim.set_setup_dir(ProjectSettings.globalize_path("user://setups"))

	settings = Settings.new()
	add_child(settings)
	reader = InputReader.new()
	add_child(reader)
	audio = Audio.new()
	add_child(audio)

	# Lighting: a mid-afternoon sun with long-range shadows, under a generated
	# sky with a sun disc and a thin layer of cloud. All made here; no images.
	sun = DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-36.0, 35.0, 0.0)
	sun.light_color = Color(1.0, 0.95, 0.86)
	sun.light_energy = 1.35
	sun.directional_shadow_mode = DirectionalLight3D.SHADOW_PARALLEL_4_SPLITS
	sun.directional_shadow_max_distance = 260.0
	sun.directional_shadow_blend_splits = true
	sun.shadow_blur = 1.2
	sun.light_angular_distance = 0.6
	add_child(sun)
	var horizon := Color(0.74, 0.81, 0.89)
	var clouds := NoiseTexture2D.new()
	clouds.width = 1024
	clouds.height = 512
	clouds.seamless = true
	var cloud_noise := FastNoiseLite.new()
	cloud_noise.seed = 4
	cloud_noise.frequency = 0.008
	cloud_noise.fractal_octaves = 5
	clouds.noise = cloud_noise
	var cloud_ramp := Gradient.new()
	cloud_ramp.set_offset(0, 0.52)
	cloud_ramp.set_color(0, Color(0, 0, 0))
	cloud_ramp.set_offset(1, 0.78)
	cloud_ramp.set_color(1, Color(1, 1, 1))
	clouds.color_ramp = cloud_ramp
	var sky_material := ProceduralSkyMaterial.new()
	sky_material.sky_top_color = Color(0.20, 0.40, 0.76)
	sky_material.sky_horizon_color = horizon
	sky_material.sky_curve = 0.12
	sky_material.sky_cover = clouds
	sky_material.sky_cover_modulate = Color(1.0, 1.0, 1.0, 0.55)
	sky_material.ground_horizon_color = horizon
	sky_material.ground_bottom_color = Color(0.30, 0.36, 0.28)
	sky_material.sun_angle_max = 8.0
	sky_material.sun_curve = 0.08
	var sky := Sky.new()
	sky.sky_material = sky_material
	environment = Environment.new()
	environment.background_mode = Environment.BG_SKY
	environment.sky = sky
	environment.ambient_light_source = Environment.AMBIENT_SOURCE_SKY
	environment.ambient_light_energy = 0.75
	environment.reflected_light_source = Environment.REFLECTION_SOURCE_SKY
	environment.tonemap_mode = Environment.TONE_MAPPER_ACES
	environment.tonemap_exposure = 1.0
	environment.glow_enabled = true
	environment.glow_intensity = 0.25
	environment.glow_hdr_threshold = 1.2
	environment.fog_enabled = true
	environment.fog_light_color = horizon
	environment.fog_density = 0.0006
	environment.fog_sun_scatter = 0.15
	environment.fog_aerial_perspective = 0.2
	var world_env := WorldEnvironment.new()
	world_env.environment = environment
	add_child(world_env)

	camera = Camera3D.new()
	camera.far = 3000.0
	camera.near = 0.1
	camera.position = Vector3(0.0, 6.0, 12.0)
	add_child(camera)

	settings.changed.connect(_apply_settings)
	_apply_settings()
	_show_menu()
	if "--self-test" in OS.get_cmdline_args() or "--self-test" in OS.get_cmdline_user_args():
		_self_test.call_deferred()


# The data folder sits beside the executable in a packaged build and beside
# the Godot project in the repository.
func _find_data_dir() -> String:
	var candidates := [
		OS.get_executable_path().get_base_dir().path_join("data"),
		ProjectSettings.globalize_path("res://").path_join("../data").simplify_path(),
	]
	for dir in candidates:
		if DirAccess.dir_exists_absolute(dir.path_join("cars")):
			return dir
	return ""


func _apply_settings() -> void:
	var window := get_window()
	var fullscreen: bool = settings.get_value("display/fullscreen")
	var wanted := Window.MODE_FULLSCREEN if fullscreen else Window.MODE_WINDOWED
	if (window.mode == Window.MODE_FULLSCREEN) != fullscreen:
		window.mode = wanted
	DisplayServer.window_set_vsync_mode(
		DisplayServer.VSYNC_ENABLED if settings.get_value("display/vsync") else DisplayServer.VSYNC_DISABLED)
	var msaa: int = clampi(settings.get_value("display/msaa"), 0, settings.MSAA_MODES.size() - 1)
	get_viewport().msaa_3d = settings.MSAA_MODES[msaa]
	sun.shadow_enabled = settings.get_value("display/shadows")
	environment.ssao_enabled = settings.get_value("display/ambient_occlusion")
	camera.fov = clampf(settings.get_value("display/fov"), 30.0, 110.0)
	AudioServer.set_bus_volume_db(0, linear_to_db(maxf(settings.get_value("audio/volume"), 0.0001)))


# Started with --self-test: drive the first car on the first track at half
# throttle for three seconds, print what happened and quit with 0 if the
# car moved. Used to check a packaged build on a clean machine.
func _self_test() -> void:
	var cars: PackedStringArray = sim.list_ids("cars")
	var tracks: PackedStringArray = sim.list_ids("tracks")
	var presets: PackedStringArray = sim.list_ids("ffb")
	if cars.is_empty() or tracks.is_empty() or presets.is_empty():
		printerr("self-test: no data found")
		get_tree().quit(1)
		return
	print("self-test: %s, data %s" % [sim.product_name(), _find_data_dir()])
	_start_driving(cars[0], tracks[0], true, true, presets[0], "", false)
	if not sim.is_running():
		printerr("self-test: session did not start: " + sim.last_error())
		get_tree().quit(1)
		return
	var began := Time.get_ticks_msec()
	fixed_input = {"steer": 0.0, "throttle": 0.5, "brake": 0.0, "clutch": 0.0, "handbrake": 0.0, "buttons": 0}
	while Time.get_ticks_msec() - began < 3000:
		await get_tree().process_frame
	var speed: float = sim.hud().get("speed", 0.0)
	var status: Dictionary = sim.status()
	print("self-test: %s on %s, %.1f m/s after 3 s, physics %.0f Hz" % [cars[0], tracks[0], speed, status.get("tick_rate", 0.0)])
	sim.stop_session()
	get_tree().quit(0 if speed > 2.0 else 1)


func _fatal(message: String) -> void:
	var label := Label.new()
	label.text = message
	label.set_anchors_preset(Control.PRESET_CENTER)
	var layer := CanvasLayer.new()
	layer.add_child(label)
	add_child(layer)
	push_error(message)


# The title screen is where every other screen returns to.
func _show_menu() -> void:
	_clear_session()
	_close_overlays()
	_show_scenery()
	title = Title.new()
	title.setup(sim)
	title.drive_requested.connect(_show_select)
	title.replays_requested.connect(_show_replays)
	title.controls_requested.connect(_show_input_setup)
	title.settings_requested.connect(_show_settings)
	add_child(title)


func _show_select() -> void:
	_close_overlays()
	menu = Menu.new()
	menu.setup(sim, settings)
	menu.drive_requested.connect(_start_driving)
	menu.back_requested.connect(_show_menu)
	add_child(menu)


# Builds the scenery for the title screen once: the circuit if it is
# there, otherwise the first track.
func _show_scenery() -> void:
	if scenery or sim.is_running():
		return
	var tracks: PackedStringArray = sim.list_ids("tracks")
	var cars: PackedStringArray = sim.list_ids("cars")
	if tracks.is_empty() or cars.is_empty():
		return
	var track: String = "ridgeline_circuit" if "ridgeline_circuit" in tracks else tracks[0]
	if not sim.load_scenery(cars[0], track):
		return
	scenery = World.new()
	add_child(scenery)
	scenery.build(sim)
	var paths: Array = sim.track_paths()
	scenery_points = paths[0].points if not paths.is_empty() else PackedVector3Array()
	scenery_s = 0.0


func _hide_scenery() -> void:
	if scenery:
		scenery.queue_free()
		scenery = null


# The title camera flies along the track 30 m up and to the side, looking
# ahead down the road.
func _fly_scenery(delta: float) -> void:
	var n := scenery_points.size()
	if n < 3:
		return
	scenery_s = fmod(scenery_s + delta * 9.0, float(n))
	var i := int(scenery_s)
	var at := scenery_points[i].lerp(scenery_points[(i + 1) % n], scenery_s - i)
	var ahead := scenery_points[(i + 40) % n]
	var along := (ahead - at)
	along.y = 0.0
	along = along.normalized() if along.length() > 0.01 else Vector3.FORWARD
	var side := Vector3(along.z, 0.0, -along.x)
	camera.global_position = at + side * 25.0 + Vector3.UP * 22.0
	camera.look_at(ahead + Vector3.UP * 2.0, Vector3.UP)
	camera.fov = 60.0


# Removes the title, selection, pause and results screens.
func _close_overlays() -> void:
	_close_menu()
	for node in [title, pause_menu, results]:
		if node:
			node.queue_free()
	title = null
	pause_menu = null
	results = null


func _close_menu() -> void:
	if menu:
		menu.remember_choices()
		menu.queue_free()
		menu = null


func _show_input_setup() -> void:
	_close_overlays()
	setup = InputSetup.new()
	setup.setup(sim, reader, settings)
	setup.closed.connect(func() -> void:
		setup.queue_free()
		setup = null
		_show_menu())
	add_child(setup)


func _show_replays() -> void:
	_clear_session()
	_close_overlays()
	replay_picker = ReplayPicker.new()
	replay_picker.closed.connect(func() -> void:
		replay_picker.queue_free()
		replay_picker = null
		_show_menu())
	replay_picker.watch_requested.connect(_watch_replay)
	add_child(replay_picker)


func _watch_replay(path: String) -> void:
	_hide_scenery()
	if not sim.replay_open(path):
		replay_picker.show_error(sim.last_error())
		return
	replay_picker.queue_free()
	replay_picker = null
	replay_view = ReplayView.new()
	replay_view.setup(sim, camera)
	replay_view.closed.connect(func() -> void:
		replay_view.close()
		replay_view.queue_free()
		replay_view = null
		_show_replays())
	add_child(replay_view)


func _show_settings() -> void:
	_close_overlays()
	settings_screen = SettingsScreen.new()
	settings_screen.setup(settings)
	settings_screen.closed.connect(func() -> void:
		settings_screen.queue_free()
		settings_screen = null
		_show_menu())
	add_child(settings_screen)


# Every session is recorded from its start; on leaving, the recording is
# written to the replays folder, keeping the newest few.
func _save_session_replay() -> void:
	session_replay = ""
	var frames: int = sim.status().get("replay_frames", 0)
	if frames < MIN_SESSION_REPLAY_S * 1000.0:
		return
	var dir := ProjectSettings.globalize_path("user://replays")
	DirAccess.make_dir_recursive_absolute(dir)
	var stamp := Time.get_datetime_string_from_system().replace(":", "-").replace("T", "_")
	session_replay = dir.path_join("%s_%s_%s.rpl" % [stamp, session_car, session_track])
	sim.replay_stop(session_replay)
	var names := Array(DirAccess.get_files_at(dir)).filter(func(n: String) -> bool: return n.get_extension() == "rpl")
	names.sort()
	while names.size() > KEEP_SESSION_REPLAYS:
		DirAccess.remove_absolute(dir.path_join(names.pop_front()))


func _clear_session() -> void:
	for node in [world, car_view, ghost_view, skid_marks, hud]:
		if node:
			node.queue_free()
	world = null
	car_view = null
	ghost_view = null
	skid_marks = null
	hud = null
	if audio:
		audio.stop()
	if sim and sim.is_running():
		_log_input_delay()
		_save_session_replay()
		sim.stop_session()
	telemetry_on = false
	replay_on = false
	paused = false


func _start_driving(car_id: String, track_id: String, auto_clutch: bool, auto_shift: bool, ffb_preset: String, device_id: String, direct_steering: bool) -> void:
	# Keep settings changed in the setup screen unless another preset was picked.
	if sim.ffb_config().get("id", "") != ffb_preset:
		sim.ffb_load_preset(ffb_preset)
		settings.apply_ffb(sim)
	if sim.has_method("set_traction_control"):
		sim.set_traction_control(settings.get_value("menu/traction_control"))
	_hide_scenery()
	if not sim.start_session(car_id, track_id, auto_clutch, auto_shift, ffb_preset):
		if menu:
			menu.show_error(sim.last_error())
		else:
			push_error(sim.last_error())
			_show_menu()
		return
	last_drive = [car_id, track_id, auto_clutch, auto_shift, ffb_preset, device_id, direct_steering]
	session_laps = []
	session_runs_seen = 0
	session_started_ms = Time.get_ticks_msec()
	_close_overlays()
	settings.save()
	session_car = car_id
	session_track = track_id
	sim.replay_start()
	cockpit_view = settings.get_value("driving/seat_view")
	show_ghost = settings.get_value("driving/ghost")
	map_mode = clampi(settings.get_value("driving/map_mode"), 0, 3)
	world = World.new()
	add_child(world)
	world.build(sim)
	var info: Dictionary = sim.car_info()
	car_view = CarView.new()
	add_child(car_view)
	car_view.build(info)
	ghost_view = CarView.new()
	add_child(ghost_view)
	ghost_view.build(info)
	ghost_view.make_ghost()
	audio.start(info, sim.track_info())
	skid_marks = SkidMarks.new()
	add_child(skid_marks)
	skid_marks.setup(info)
	hud = Hud.new()
	add_child(hud)
	hud.setup(sim)
	hud.set_map_mode(map_mode)
	if device_id != "":
		sim.ffb_open(device_id, DisplayServer.window_get_native_handle(DisplayServer.WINDOW_HANDLE))
		sim.ffb_set_direct_steering(direct_steering)
	# Wheel, pedals and shifter read on the physics thread at full rate.
	var direct_map: Dictionary = reader.direct_map()
	if not direct_map.axes.is_empty() or not direct_map.buttons.is_empty():
		if not sim.set_direct_inputs(direct_map):
			hud.flash(sim.last_error())


func _process(delta: float) -> void:
	if scenery and not sim.is_running():
		_fly_scenery(delta)
	if sim == null or not sim.is_running():
		return
	var input: Dictionary = fixed_input if not fixed_input.is_empty() else reader.read(delta)
	sim.set_input(input.steer, input.throttle, input.brake, input.clutch, input.handbrake, input.buttons, -128)
	var car_xform: Transform3D = sim.car_transform()
	var values: Dictionary = sim.hud()
	_note_laps(values)
	car_view.update(car_xform, sim.wheel_states(), values.get("steering_wheel_angle", 0.0))
	world.update_cones(sim.cones_down())
	skid_marks.update(car_xform, sim.wheel_states(), values)
	var ghost: Dictionary = sim.ghost()
	values["ghost_time"] = ghost.get("time", 0.0)
	values["ghost_setup"] = ghost.get("setup", "")
	values["ghost_has_delta"] = ghost.get("has_delta", false)
	values["ghost_delta"] = ghost.get("delta", 0.0)
	_update_ghost(ghost, car_xform)
	hud.update_map(car_xform, ghost_view.visible, ghost_view.global_transform)
	hud.update(values, sim.status(), input, telemetry_on, replay_on)
	audio.update(values, input, delta, paused)
	_update_camera(car_xform, delta)


func _update_ghost(ghost: Dictionary, car_xform: Transform3D) -> void:
	var shown: bool = show_ghost and ghost.get("visible", false)
	if shown:
		var xform: Transform3D = ghost.transform
		if cockpit_view and xform.origin.distance_to(car_xform.origin) < GHOST_HIDE_DISTANCE:
			shown = false
		else:
			ghost_view.update(xform, ghost.wheels)
	ghost_view.visible = shown


func _update_camera(car_xform: Transform3D, delta: float) -> void:
	if cockpit_view:
		var seat: Vector3 = car_view.seat_offset + Vector3.UP * settings.get_value("driving/seat_height")
		camera.global_transform = car_xform * Transform3D(Basis.IDENTITY, seat)
		car_view.set_seat_view(true)
		return
	car_view.set_seat_view(false)
	# Chase camera: behind and above, easing toward its target.
	var back := car_xform.basis.z
	back.y = 0.0
	back = back.normalized() if back.length() > 0.01 else Vector3.BACK
	var target := car_xform.origin + back * 7.5 + Vector3.UP * 2.8
	var blend: float = 1.0 - exp(-6.0 * delta)
	camera.global_position = camera.global_position.lerp(target, blend)
	camera.look_at(car_xform.origin + Vector3.UP * 0.6, Vector3.UP)


func _unhandled_key_input(event: InputEvent) -> void:
	if not event.pressed or event.echo or sim == null or not sim.is_running():
		return
	match event.physical_keycode:
		KEY_ESCAPE:
			_open_pause_menu()
		KEY_C:
			cockpit_view = not cockpit_view
		KEY_G:
			show_ghost = not show_ghost
		KEY_M:
			audio.toggle_mute()
		KEY_N:
			map_mode = (map_mode + 1) % 4
			hud.set_map_mode(map_mode)
			settings.set_value("driving/map_mode", map_mode)
			settings.save()
		KEY_P:
			paused = not paused
			sim.set_paused(paused)
		KEY_F1:
			hud.toggle_debug()
		KEY_F2:
			telemetry_on = not telemetry_on
			if telemetry_on:
				sim.telemetry_start(_recording_path("telemetry", "csv"), 100.0)
			else:
				sim.telemetry_stop()
		KEY_F3:
			# The session is always being recorded; F3 keeps a copy of it so far.
			sim.replay_save(_recording_path("replay", "rpl"))
			hud.flash("replay saved")
		KEY_H:
			hud.visible = not hud.visible
		KEY_F4:
			hud.toggle_ffb_meter()
		KEY_F5:
			var state: String = sim.status().get("ffb_log_state", "off")
			if state == "waiting" or state == "recording":
				sim.ffb_log_stop()
			else:
				sim.ffb_log_start(_recording_path("ffb_" + session_car + "_" + session_track, "csv"))
				hud.flash("FFB log armed: it records the next full lap")
		KEY_BRACKETLEFT:
			_nudge_ffb("gain", -0.01, 0.0, 1.5)
		KEY_BRACKETRIGHT:
			_nudge_ffb("gain", 0.01, 0.0, 1.5)
		KEY_SEMICOLON:
			_nudge_ffb("min_force", -0.01, 0.0, 0.3)
		KEY_APOSTROPHE:
			_nudge_ffb("min_force", 0.01, 0.0, 0.3)


# Live force-feedback adjustment while the tuning meter is showing. The
# change is remembered for the preset, as on the Controls screen.
func _nudge_ffb(name: String, step: float, lowest: float, highest: float) -> void:
	if not hud.ffb_meter_visible():
		return
	var config: Dictionary = sim.ffb_config()
	var value: float = clamp(snappedf(config.get(name, 0.0) + step, 0.001), lowest, highest)
	if sim.ffb_set(name, value):
		settings.set_ffb(config.get("id", ""), name, value)
		settings.save()
		hud.flash("%s %.2f" % [name.replace("_", " "), value])


func _open_pause_menu() -> void:
	if pause_menu:
		return
	sim.set_paused(true)
	pause_menu = PauseMenu.new()
	pause_menu.resume_requested.connect(func() -> void:
		pause_menu.queue_free()
		pause_menu = null
		sim.set_paused(paused))
	pause_menu.restart_requested.connect(_restart_session)
	pause_menu.end_requested.connect(_end_session)
	pause_menu.quit_requested.connect(_show_menu)
	add_child(pause_menu)


# A fresh session with the same car, track and settings.
func _restart_session() -> void:
	var again := last_drive.duplicate()
	_clear_session()
	_close_overlays()
	_start_driving.callv(again)


# Leaves the session and shows what happened in it.
func _end_session() -> void:
	var car_name: String = sim.car_info().get("name", "")
	var track_name: String = sim.track_info().get("name", "")
	var ghost_time: float = sim.ghost().get("time", 0.0)
	var ghost_setup: String = sim.ghost().get("setup", "")
	var setup_name: String = sim.session_setup_name()
	_note_laps(sim.hud())
	var duration := (Time.get_ticks_msec() - session_started_ms) / 1000.0
	var invalid: int = sim.hud().get("invalid_runs", 0)
	var laps := session_laps.duplicate()
	_clear_session()
	_close_overlays()
	_show_scenery()
	results = Results.new()
	results.setup({
		"car": car_name, "track": track_name, "duration": duration, "laps": laps,
		"invalid": invalid, "ghost_time": ghost_time, "replay": session_replay,
		"setup": setup_name, "ghost_setup": ghost_setup,
	})
	results.replay_requested.connect(func(path: String) -> void:
		_show_replays()
		_watch_replay(path))
	results.again_requested.connect(func() -> void:
		_close_overlays()
		_start_driving.callv(last_drive.duplicate()))
	results.title_requested.connect(_show_menu)
	add_child(results)


# Records each lap or run the core counted, as it is counted.
func _note_laps(values: Dictionary) -> void:
	var runs: int = values.get("runs", 0)
	if runs > session_runs_seen:
		session_laps.append({"time": values.get("last_time", 0.0), "cones": values.get("last_cones_hit", 0)})
		session_runs_seen = runs


# Appends the input delay the core measured to a log, one line per
# session, so the per-frame and physics-thread paths can be compared.
func _log_input_delay() -> void:
	var s: Dictionary = sim.status()
	if not s.has("input_delay_frame_ms"):
		return
	var dir := ProjectSettings.globalize_path("user://recordings")
	DirAccess.make_dir_recursive_absolute(dir)
	var path := dir.path_join("input_delay.log")
	var file := FileAccess.open(path, FileAccess.READ_WRITE if FileAccess.file_exists(path) else FileAccess.WRITE)
	if file == null:
		return
	file.seek_end()
	file.store_line("%s  %s on %s  fps %.0f  direct %s  frame path %.2f ms (max %.2f)  device read %.3f ms (max %.3f)  steering lag %s" % [
		Time.get_datetime_string_from_system(), session_car, session_track, Engine.get_frames_per_second(),
		"on" if s.get("direct_inputs", false) else "off",
		s.get("input_delay_frame_ms", 0.0), s.get("input_delay_frame_max_ms", 0.0),
		s.get("input_delay_direct_ms", 0.0), s.get("input_delay_direct_max_ms", 0.0),
		("%.1f ms" % s.get("steer_lag_ms", 0.0)) if s.get("steer_lag_measured", false) else "not measured"])


func _recording_path(kind: String, extension: String) -> String:
	var stamp := Time.get_datetime_string_from_system().replace(":", "-")
	var dir := ProjectSettings.globalize_path("user://recordings")
	DirAccess.make_dir_recursive_absolute(dir)
	return dir.path_join("%s_%s.%s" % [kind, stamp, extension])


func _exit_tree() -> void:
	if sim and sim.is_running():
		sim.stop_session()
