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

var sim  # SimBridge, from the binding library
var reader
var menu
var setup
var world
var car_view
var hud
var camera: Camera3D
var cockpit_view := false
var paused := false
var telemetry_on := false
var replay_on := false


func _ready() -> void:
	if not ClassDB.class_exists("SimBridge"):
		_fatal("The simulation library is not loaded.\nBuild it first (see README), then restart.")
		return
	sim = ClassDB.instantiate("SimBridge")
	get_window().title = sim.product_name()
	# The data directory sits next to the Godot project in the repository.
	var data_dir := ProjectSettings.globalize_path("res://").path_join("../data").simplify_path()
	if not sim.set_data_dir(data_dir):
		_fatal("Data directory not found:\n%s" % data_dir)
		return

	reader = InputReader.new()
	add_child(reader)

	var sun := DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-55.0, 35.0, 0.0)
	add_child(sun)
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.62, 0.74, 0.86)
	env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.ambient_light_color = Color(0.75, 0.78, 0.82)
	env.ambient_light_energy = 0.7
	var world_env := WorldEnvironment.new()
	world_env.environment = env
	add_child(world_env)

	camera = Camera3D.new()
	camera.far = 3000.0
	camera.position = Vector3(0.0, 6.0, 12.0)
	add_child(camera)

	_show_menu()


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
	menu.setup(sim)
	menu.drive_requested.connect(_start_driving)
	menu.input_setup_requested.connect(_show_input_setup)
	add_child(menu)


func _show_input_setup() -> void:
	if menu:
		menu.queue_free()
		menu = null
	setup = InputSetup.new()
	setup.setup(sim, reader)
	setup.closed.connect(func() -> void:
		setup.queue_free()
		setup = null
		_show_menu())
	add_child(setup)


func _clear_session() -> void:
	for node in [world, car_view, hud]:
		if node:
			node.queue_free()
	world = null
	car_view = null
	hud = null
	if sim and sim.is_running():
		sim.stop_session()
	telemetry_on = false
	replay_on = false
	paused = false


func _start_driving(car_id: String, track_id: String, auto_clutch: bool, auto_shift: bool, ffb_preset: String, device_id: String, direct_steering: bool) -> void:
	# Keep settings changed in the setup screen unless another preset was picked.
	if sim.ffb_config().get("id", "") != ffb_preset:
		sim.ffb_load_preset(ffb_preset)
	if not sim.start_session(car_id, track_id, auto_clutch, auto_shift, ffb_preset):
		menu.show_error(sim.last_error())
		return
	menu.queue_free()
	menu = null
	world = World.new()
	add_child(world)
	world.build(sim)
	car_view = CarView.new()
	add_child(car_view)
	car_view.build(sim.car_info())
	hud = Hud.new()
	add_child(hud)
	hud.setup(sim)
	if device_id != "":
		sim.ffb_open(device_id, DisplayServer.window_get_native_handle(DisplayServer.WINDOW_HANDLE))
		sim.ffb_set_direct_steering(direct_steering)


func _process(delta: float) -> void:
	if sim == null or not sim.is_running():
		return
	var input: Dictionary = reader.read(delta)
	sim.set_input(input.steer, input.throttle, input.brake, input.clutch, input.handbrake, input.buttons, -128)
	var car_xform: Transform3D = sim.car_transform()
	car_view.update(car_xform, sim.wheel_states())
	world.update_cones(sim.cones_down())
	hud.update(sim.hud(), sim.status(), input, telemetry_on, replay_on)
	_update_camera(car_xform, delta)


func _update_camera(car_xform: Transform3D, delta: float) -> void:
	if cockpit_view:
		camera.global_transform = car_xform * Transform3D(Basis.IDENTITY, Vector3(-0.35, 0.55, 0.3))
		return
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
