# SPDX-License-Identifier: Apache-2.0
# Checks photo mode and the trailer director in the running game (D-082).
# Drives a few seconds, opens photo mode, checks the session is frozen,
# captures a still at twice the window size with depth of field; then
# leaves (saving the replay), opens the replay, and exports a marked
# stretch as a PNG sequence. Needs a display.
#   godot --path godot --script res://tests/photo_trailer.gd -- OUTPUT_PREFIX
extends SceneTree


class FixedInput:
	extends Node
	func read(_delta: float) -> Dictionary:
		return {"steer": 0.05, "throttle": 0.7, "brake": 0.0, "clutch": 0.0, "handbrake": 0.0, "buttons": 0}


var failures: Array = []


func _check(ok: bool, what: String) -> void:
	print(("ok    " if ok else "FAIL  ") + what)
	if not ok:
		failures.append(what)


func _initialize() -> void:
	_run.call_deferred()


func _frames(n: int) -> void:
	for i in n:
		await process_frame


func _run() -> void:
	var prefix: String = OS.get_cmdline_user_args()[0]
	var main = load("res://main.tscn").instantiate()
	root.add_child(main)
	await _frames(3)
	var presets: PackedStringArray = main.sim.list_ids("ffb")
	main._start_driving("hatch_tern_gt", "ridgeline_circuit", true, true, presets[0], "", false)
	main.reader = FixedInput.new()
	var began := Time.get_ticks_msec()
	while Time.get_ticks_msec() - began < 5000:
		await process_frame

	# Photo mode freezes the session.
	main._open_photo_mode()
	await _frames(5)
	var photo = main.photo_mode
	_check(photo != null, "photo mode opens")
	var tick_a: int = main.sim.hud().get("tick", 0)
	await _frames(30)
	var tick_b: int = main.sim.hud().get("tick", 0)
	_check(tick_a == tick_b, "the session does not advance in photo mode (%d, %d)" % [tick_a, tick_b])
	_check(not main.hud.visible, "the HUD is hidden")
	photo.camera.global_position += Vector3(2.0, 1.0, 0.0)
	photo.set_view(35.0, 8.0, 0.3, true)
	photo.scale_index = 1
	await photo.capture()
	var image := Image.load_from_file(photo.last_saved)
	var window := root.get_viewport().get_visible_rect().size
	_check(image != null and image.get_width() == int(window.x * 2.0), "a 2x still is saved: %s (%dx%d, window %dx%d)" % [
		photo.last_saved, image.get_width() if image else 0, image.get_height() if image else 0, window.x, window.y])
	if image:
		image.save_png(prefix + "_photo.png")
	var photo_path: String = photo.last_saved
	photo.leave()
	await _frames(3)
	_check(main.photo_mode == null and main.hud.visible, "leaving photo mode restores the HUD")
	await _frames(30)
	var tick_c: int = main.sim.hud().get("tick", 0)
	_check(tick_c > tick_b, "the session runs again after photo mode")

	# Save a copy of the recording outside the player's replays folder (the
	# session is too short to be kept there, so nothing of theirs is
	# trimmed), leave, open the copy and export a stretch.
	var path := OS.get_temp_dir().path_join("photo_trailer_test.rpl")
	main.sim.replay_save(path)
	await _frames(10)
	main._show_menu()
	await process_frame
	main._show_replays()
	await process_frame
	main._watch_replay(path)
	await _frames(2)
	var view = main.replay_view
	if view == null:
		_check(false, "the replay opens: %s (%s)" % [path, main.sim.last_error()])
		quit(1)
		return
	began = Time.get_ticks_msec()
	while not view.prepared and Time.get_ticks_msec() - began < 30000:
		await process_frame
	view.mark_in = 1.0
	view.mark_out = 3.0
	var out: String = await view.export_sequence(15, 0.5)
	var files := Array(DirAccess.get_files_at(out))
	var pngs := files.filter(func(f: String) -> bool: return f.ends_with(".png"))
	_check(pngs.size() == 31, "31 frames for 2 s at 15 fps (%d)" % pngs.size())
	var shots = JSON.parse_string(FileAccess.get_file_as_string(out.path_join("shots.json")))
	_check(shots != null and shots.shots.size() == 1 and shots.frames == 31, "shot list written")
	var cuts: Array = view.shot_list(0.0, 20.0)
	_check(cuts.size() >= 5 and cuts[0][0] == "trackside" and cuts[1][0] == "chase_low", "the director cuts between shots: %s" % str(cuts.slice(0, 5)))
	var first := Image.load_from_file(out.path_join("frame_00001.png"))
	_check(first != null and first.get_width() == int(window.x * 0.5), "frames at the asked size")
	if first:
		first.save_png(prefix + "_trailer_frame.png")
	var info: Dictionary = main.sim.replay_info()
	_check(info.get("mismatch_tick", -1) < 0, "the replay still reproduces exactly")
	print("trailer sequence in ", out)
	view.close()
	# Leave no test files behind among the player's own.
	DirAccess.remove_absolute(path)
	for f in files:
		DirAccess.remove_absolute(out.path_join(f))
	DirAccess.remove_absolute(out)
	DirAccess.remove_absolute(photo_path)
	# The folders too, if the test made them and they are empty.
	DirAccess.remove_absolute(photo_path.get_base_dir())
	DirAccess.remove_absolute(out.get_base_dir())
	print("FAILURES: %d" % failures.size())
	quit(0 if failures.is_empty() else 1)
