# SPDX-License-Identifier: Apache-2.0
# Headless smoke test of the front end and the binding. Run with:
#   godot --headless --path godot --script res://tests/smoke.gd
# It loads every car on every track, drives for a moment, builds the
# placeholder geometry and HUD, and exits non-zero on any failure. It cannot
# check what the picture looks like or how a wheel feels.
extends SceneTree

const World := preload("res://scripts/world.gd")
const CarView := preload("res://scripts/car_view.gd")
const Hud := preload("res://scripts/hud.gd")
const InputReader := preload("res://scripts/input_reader.gd")
const Menu := preload("res://scripts/menu.gd")
const InputSetup := preload("res://scripts/input_setup.gd")

var failures := 0


func check(condition: bool, message: String) -> void:
	if not condition:
		failures += 1
		printerr("FAIL: " + message)


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	check(ClassDB.class_exists("SimBridge"), "SimBridge class is registered")
	if failures > 0:
		quit(1)
		return
	var sim = ClassDB.instantiate("SimBridge")
	var data_dir := ProjectSettings.globalize_path("res://").path_join("../data").simplify_path()
	check(sim.set_data_dir(data_dir), "data directory found at " + data_dir)
	check(sim.product_name() != "", "product name comes from the core")
	var cars: PackedStringArray = sim.list_ids("cars")
	var tracks: PackedStringArray = sim.list_ids("tracks")
	var presets: PackedStringArray = sim.list_ids("ffb")
	check(cars.size() >= 2, "at least two cars")
	check(tracks.size() == 3, "three tracks")
	check(presets.size() >= 1, "a force-feedback preset")
	check(sim.describe("cars", cars[0]).get("name", "") != "", "car has a name")
	check(sim.ffb_devices() is Array, "device enumeration returns a list")

	# The menu and setup screens build without errors.
	var reader = InputReader.new()
	root.add_child(reader)
	var menu = Menu.new()
	menu.setup(sim)
	root.add_child(menu)
	await process_frame
	menu.queue_free()
	check(sim.ffb_load_preset(presets[0]), "preset loads")
	var setup = InputSetup.new()
	setup.setup(sim, reader)
	root.add_child(setup)
	await process_frame
	await process_frame
	setup.queue_free()
	check(sim.ffb_set("gain", 0.25), "gain can be changed")
	check(not sim.ffb_set("soft_clip_knee", 2.0), "an invalid setting is rejected")
	check(absf(sim.ffb_config().gain - 0.25) < 1e-9, "the change is visible")

	for car in cars:
		for track in tracks:
			var label := "%s on %s" % [car, track]
			check(sim.start_session(car, track, true, true, presets[0]), label + " starts: " + sim.last_error())
			if not sim.is_running():
				continue
			var world = World.new()
			root.add_child(world)
			world.build(sim)
			var view = CarView.new()
			root.add_child(view)
			view.build(sim.car_info())
			var hud = Hud.new()
			root.add_child(hud)
			hud.setup(sim)
			hud.toggle_debug()
			var start: Transform3D = sim.car_transform()
			var input := {"steer": 0.05, "throttle": 1.0, "brake": 0.0, "clutch": 0.0, "handbrake": 0.0, "buttons": 0}
			var began := Time.get_ticks_msec()
			while Time.get_ticks_msec() - began < 1500:
				sim.set_input(input.steer, input.throttle, input.brake, input.clutch, input.handbrake, input.buttons, -128)
				view.update(sim.car_transform(), sim.wheel_states())
				world.update_cones(sim.cones_down())
				hud.update(sim.hud(), sim.status(), input, false, false)
				await process_frame
			var h: Dictionary = sim.hud()
			var moved: float = sim.car_transform().origin.distance_to(start.origin)
			check(h.get("tick", 0) > 300, label + ": physics advanced (%d ticks)" % h.get("tick", 0))
			check(h.get("speed", 0.0) > 1.0, label + ": car is moving (%.2f m/s)" % h.get("speed", 0.0))
			check(moved > 0.5, label + ": drawn car moved (%.2f m)" % moved)
			check(sim.wheel_states().size() == 20, label + ": four wheel states")
			check(h.load.size() == 4 and h.temp_surface.size() == 4, label + ": per-wheel debug values")
			# Forward in the simulation must be -Z in Godot.
			var info: Dictionary = sim.track_info()
			if absf(info.spawn_heading) < 0.01:
				check(sim.car_transform().origin.z < start.origin.z, label + ": drives toward -Z")
			check(world.get_child_count() > 0, label + ": world geometry was built")
			check(sim.status().get("error", "") == "", label + ": no host error")
			world.queue_free()
			view.queue_free()
			hud.queue_free()
			sim.stop_session()
			await process_frame

	if failures == 0:
		print("smoke test passed: %d cars x %d tracks" % [cars.size(), tracks.size()])
	else:
		printerr("smoke test FAILED with %d problem(s)" % failures)
	quit(1 if failures > 0 else 0)
