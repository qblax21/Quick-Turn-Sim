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
const Title := preload("res://scripts/title.gd")
const PauseMenu := preload("res://scripts/pause_menu.gd")
const Results := preload("res://scripts/results.gd")
const Garage := preload("res://scripts/garage.gd")
const InputSetup := preload("res://scripts/input_setup.gd")
const Settings := preload("res://scripts/settings.gd")
const SettingsScreen := preload("res://scripts/settings_screen.gd")

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
	check(tracks.size() >= 4, "four tracks")
	check(presets.size() >= 1, "a force-feedback preset")
	check(sim.describe("cars", cars[0]).get("name", "") != "", "car has a name")
	check(sim.ffb_devices() is Array, "device enumeration returns a list")

	# The menu and setup screens build without errors.
	var reader = InputReader.new()
	root.add_child(reader)
	# Settings are read but never saved here, so a test run leaves the
	# player's file alone.
	var settings = Settings.new()
	root.add_child(settings)
	var menu = Menu.new()
	menu.setup(sim, settings)
	root.add_child(menu)
	await process_frame
	menu.queue_free()
	for screen_script in [Title, PauseMenu, Results]:
		var s = screen_script.new()
		if s.has_method("setup"):
			if screen_script == Title:
				s.setup(sim)
			else:
				s.setup({"car": "car", "track": "track", "duration": 60.0, "laps": [{"time": 61.2, "cones": 0}, {"time": 60.4, "cones": 1}], "invalid": 1, "ghost_time": 59.9, "replay": ""})
		root.add_child(s)
		await process_frame
		s.queue_free()
	check(true, "title, pause and results screens build")
	check(sim.ffb_load_preset(presets[0]), "preset loads")
	var setup = InputSetup.new()
	setup.setup(sim, reader, settings)
	root.add_child(setup)
	await process_frame
	await process_frame
	setup.queue_free()
	var screen = SettingsScreen.new()
	screen.setup(settings)
	root.add_child(screen)
	await process_frame
	screen.queue_free()
	check(settings.get_value("display/fov") > 0.0, "settings have defaults")
	check(sim.ffb_set("gain", 0.25), "gain can be changed")
	check(not sim.ffb_set("soft_clip_knee", 2.0), "an invalid setting is rejected")
	check(absf(sim.ffb_config().gain - 0.25) < 1e-9, "the change is visible")
	# Setups: ranges from the core, saved, loaded back and used.
	sim.set_setup_dir(OS.get_temp_dir().path_join("qts_smoke_setups"))
	var setup_items: Array = sim.setup_items("hatch_tern_gt")
	check(setup_items.size() >= 15, "the garage offers adjustments (%d)" % setup_items.size())
	var camber := {"front.camber_deg": -2.5}
	check(sim.setup_save("hatch_tern_gt", "test_track", "smoke test", camber), "a setup saves: " + sim.last_error())
	check("smoke test" in sim.setup_list("hatch_tern_gt", "test_track"), "a saved setup is listed")
	var back: Dictionary = sim.setup_load("hatch_tern_gt", "test_track", "smoke test")
	check(absf(back.get("values", {}).get("front.camber_deg", 0.0) + 2.5) < 1e-9, "a setup loads back")
	check(not sim.setup_save("hatch_tern_gt", "test_track", "too far", {"front.camber_deg": -9.0}), "an out-of-range setup is refused")
	check(sim.setup_use("hatch_tern_gt", "smoke test", back.values), "a setup can be used")
	check(sim.start_session("hatch_tern_gt", "test_track", true, true, presets[0]), "a session starts with a setup: " + sim.last_error())
	check(sim.session_setup_name() == "smoke test", "the session knows its setup")
	sim.stop_session()
	check(sim.setup_delete("hatch_tern_gt", "test_track", "smoke test"), "a setup can be deleted")
	sim.setup_use("hatch_tern_gt", "", {})
	var garage = Garage.new()
	garage.setup(sim, settings, "hatch_tern_gt", "test_track")
	root.add_child(garage)
	await process_frame
	garage.queue_free()
	var scanned: Array = sim.input_scan()
	for d in scanned:
		check(d.axes.size() == 8, "device %s reports eight axes" % d.name)
	sim.input_scan_close()
	print("devices read directly: %d" % scanned.size())
	check(sim.ffb_set("min_force", 0.05), "minimum force can be set")
	check(not sim.ffb_set("min_force", 0.6), "too much minimum force is rejected")
	check(sim.ffb_set("min_force", 0.0), "minimum force can be cleared")

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
			var ghost_view = CarView.new()
			root.add_child(ghost_view)
			ghost_view.build(sim.car_info())
			ghost_view.make_ghost()
			check(sim.ghost().has("visible"), label + " reports a ghost state")
			ghost_view.queue_free()
			var hud = Hud.new()
			root.add_child(hud)
			hud.setup(sim)
			hud.toggle_debug()
			hud.toggle_ffb_meter()
			sim.ffb_log_start(OS.get_temp_dir().path_join("qts_smoke_ffb.csv"))
			check(sim.set_direct_inputs({"axes": [], "buttons": []}), label + ": an empty direct map is accepted")
			var bogus := {"axes": [{"action": "throttle", "device": "00000000000000000000000000000001", "axis": 1}], "buttons": []}
			check(not sim.set_direct_inputs(bogus), label + ": a missing device is reported")
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
			check(sim.status().has("input_delay_frame_ms"), label + ": input delay reported")
			check(sim.hud().has("input_throttle"), label + ": controls used are reported")
			var log_state: String = sim.status().get("ffb_log_state", "")
			check(log_state in ["waiting", "recording"], label + ": FFB log armed (%s)" % log_state)
			check(sim.status().has("peak_requested"), label + ": peak force reported")
			sim.ffb_log_stop()
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
