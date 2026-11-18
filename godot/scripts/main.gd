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

# In the seat view the ghost is hidden when it is this close, so it never
# fills the windscreen.
const GHOST_HIDE_DISTANCE := 5.0

var sim  # SimBridge, from the binding library
var settings
var reader
var menu
var setup
var settings_screen
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


func _show_menu() -> void:
	_clear_session()
	menu = Menu.new()
	menu.setup(sim, settings)
	menu.drive_requested.connect(_start_driving)
	menu.input_setup_requested.connect(_show_input_setup)
	menu.settings_requested.connect(_show_settings)
	add_child(menu)


func _close_menu() -> void:
	if menu:
		menu.remember_choices()
		menu.queue_free()
		menu = null


func _show_input_setup() -> void:
	_close_menu()
	setup = InputSetup.new()
	setup.setup(sim, reader, settings)
	setup.closed.connect(func() -> void:
		setup.queue_free()
		setup = null
		_show_menu())
	add_child(setup)


func _show_settings() -> void:
	_close_menu()
	settings_screen = SettingsScreen.new()
	settings_screen.setup(settings)
	settings_screen.closed.connect(func() -> void:
		settings_screen.queue_free()
		settings_screen = null
		_show_menu())
	add_child(settings_screen)


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
	if not sim.start_session(car_id, track_id, auto_clutch, auto_shift, ffb_preset):
		menu.show_error(sim.last_error())
		return
	_close_menu()
	settings.save()
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


func _process(delta: float) -> void:
	if sim == null or not sim.is_running():
		return
	var input: Dictionary = fixed_input if not fixed_input.is_empty() else reader.read(delta)
	sim.set_input(input.steer, input.throttle, input.brake, input.clutch, input.handbrake, input.buttons, -128)
	var car_xform: Transform3D = sim.car_transform()
	var values: Dictionary = sim.hud()
	car_view.update(car_xform, sim.wheel_states(), values.get("steering_wheel_angle", 0.0))
	world.update_cones(sim.cones_down())
	skid_marks.update(car_xform, sim.wheel_states(), values)
	var ghost: Dictionary = sim.ghost()
	values["ghost_time"] = ghost.get("time", 0.0)
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
			_show_menu()
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
			replay_on = not replay_on
			if replay_on:
				sim.replay_start()
			else:
				sim.replay_stop(_recording_path("replay", "rpl"))


func _recording_path(kind: String, extension: String) -> String:
	var stamp := Time.get_datetime_string_from_system().replace(":", "-")
	var dir := ProjectSettings.globalize_path("user://recordings")
	DirAccess.make_dir_recursive_absolute(dir)
	return dir.path_join("%s_%s.%s" % [kind, stamp, extension])


func _exit_tree() -> void:
	if sim and sim.is_running():
		sim.stop_session()
