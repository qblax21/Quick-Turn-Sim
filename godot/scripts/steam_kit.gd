# SPDX-License-Identifier: Apache-2.0
# The store page kit (D-129): from replays of the computer driver, renders
# the five 1920x1080 screenshots a Steam store page needs and the frames of
# a trailer at 60 fps, all in-game footage drawn by the replay viewer's
# camera director (D-082). tools\steam_kit.ps1 records the replays, runs
# this and turns the frames into a video.
#
#   -- --steam-kit=OUT_DIR --ridgeline=R.rpl --ridgeline-from=S
#      --harrow=FOLLOW.rpl --harrow-lead=LEAD.rpl --harrow-from=S
#      [--ridgeline-seconds=18] [--harrow-seconds=20] [--kit-step=1]
#      [--kit-stills-only=1] [--kit-preset=Low|Medium|High]
#
# The trailer is a Ridgeline hot lap, then Harrow in tandem: the follower
# replay with the lead replay (a second computer car started 30 m ahead, as
# the game's tandem mode starts it) drawn beside it. --kit-step=N renders
# every Nth frame only, for a quick look.
extends Node

const ReplayView := preload("res://scripts/replay_view.gd")
const CarView := preload("res://scripts/car_view.gd")
const Capture := preload("res://scripts/capture.gd")

const SIZE := Vector2i(1920, 1080)
const FPS := 60
# Ridgeline screenshots: [seconds into the segment, shot].
const RIDGELINE_STILLS := [[5.0, "chase_low"], [12.0, "helicopter"]]
# Harrow screenshots: the shots, at the moments the follower slides most
# with the lead within TANDEM_CLOSE_M, at least STILL_GAP_S apart.
const HARROW_STILL_SHOTS := ["tandem", "trackside", "tandem_high"]
const TANDEM_CLOSE_M := 28.0
const STILL_GAP_S := 3.0
# The Harrow segment's shots in turn, and how long each lasts, s.
const TANDEM_SHOTS := ["tandem", "trackside", "tandem_high", "side_track", "tandem"]
const TANDEM_LENGTHS := [4.0, 3.5, 4.5, 3.0, 5.0]

var sim
var camera: Camera3D
var out_dir := ""
var frame_no := 0
var still_no := 0
var step := 1
var stills_only := false


func _arg(name: String, default: String = "") -> String:
	for a in OS.get_cmdline_user_args():
		if a.begins_with("--%s=" % name):
			return a.get_slice("=", 1)
	return default


# Renders everything; returns the process exit code.
func run(bridge, scene_camera: Camera3D, out: String) -> int:
	sim = bridge
	camera = scene_camera
	out_dir = out
	step = maxi(int(_arg("kit-step", "1")), 1)
	stills_only = _arg("kit-stills-only", "") == "1"
	for sub in ["screenshots", "trailer"]:
		DirAccess.make_dir_recursive_absolute(out_dir.path_join(sub))
	var ridge_from := float(_arg("ridgeline-from", "0"))
	var ridge_len := float(_arg("ridgeline-seconds", "18"))
	var harrow_from := float(_arg("harrow-from", "0"))
	var harrow_len := float(_arg("harrow-seconds", "20"))
	# The lead car's poses first: only one replay is open at a time.
	var lead := await _sample(_arg("harrow-lead"), harrow_from, harrow_len)
	if lead.is_empty():
		printerr("steam kit: could not read the lead replay: ", sim.last_error())
		return 1
	if not await _segment("ridgeline", _arg("ridgeline"), ridge_from, ridge_len, []):
		return 1
	if not await _segment("harrow", _arg("harrow"), harrow_from, harrow_len, lead):
		return 1
	print("steam kit: %d trailer frames and %d screenshots in %s" % [frame_no, still_no, out_dir])
	return 0


# Opens a replay and waits until it is prepared up to `to` seconds.
func _open(path: String, to: float) -> bool:
	if not sim.replay_open(path):
		return false
	while sim.replay_info().get("prepared", 0.0) < minf(to, sim.replay_info().get("duration", 0.0) - 0.01):
		sim.replay_prepare(20000)
		await get_tree().process_frame
	return true


# The poses of a replay at every trailer frame from `from` for `length` s.
func _sample(path: String, from: float, length: float) -> Array:
	var out: Array = []
	if not await _open(path, from + length + 1.0):
		return out
	for k in int(length * FPS):
		out.append(sim.replay_frame(from + float(k) / FPS))
	sim.replay_close()
	return out


# One trailer segment: the replay through the director's shots, with the
# lead car if there is one, plus this segment's screenshots.
func _segment(name: String, path: String, from: float, length: float, lead: Array) -> bool:
	if not await _open(path, from + length + 1.0):
		printerr("steam kit: could not open %s: %s" % [path, sim.last_error()])
		return false
	var view = ReplayView.new()
	view.setup(sim, camera)
	add_child(view)
	await get_tree().process_frame
	view.layer.visible = false
	view.playing = false
	var lead_view = null
	if not lead.is_empty():
		lead_view = CarView.new()
		add_child(lead_view)
		lead_view.build(sim.car_info())
	var tandem := not lead.is_empty()
	var shots: Array = _tandem_shots(from, from + length) if tandem else view.shot_list(from, from + length)
	var shot := 0
	for k in range(0, int(length * FPS) if not stills_only else 0, step):
		var t := from + float(k) / FPS
		while shot < shots.size() - 1 and t >= shots[shot][2]:
			shot += 1
			view.post_index = -1
		var xform := _pose(view, lead_view, lead, k, t)
		_camera(view, shots[shot][0], xform, lead_view, t - shots[shot][1])
		var image: Image = await Capture.render_size(self, camera, SIZE)
		frame_no += 1
		image.save_png(out_dir.path_join("trailer/frame_%05d.png" % frame_no))
	var stills: Array = _tandem_stills(lead, from, length) if tandem else []
	if not tandem:
		for r in RIDGELINE_STILLS:
			stills.append([int(r[0] * FPS), r[1]])
	for s in stills:
		var t: float = from + float(s[0]) / FPS
		var xform := _pose(view, lead_view, lead, s[0], t)
		view.post_index = -1
		_camera(view, s[1], xform, lead_view, 1.0)
		var image: Image = await Capture.render_size(self, camera, SIZE)
		still_no += 1
		image.save_png(out_dir.path_join("screenshots/screenshot_%d_%s.png" % [still_no, name]))
	if lead_view:
		lead_view.queue_free()
	view.close()
	view.queue_free()
	await get_tree().process_frame
	return true


# The Harrow shot list: `[kind, start, end]`, as the director's.
func _tandem_shots(from: float, to: float) -> Array:
	var shots: Array = []
	var t := from
	var k := 0
	while t < to - 0.05:
		var length: float = TANDEM_LENGTHS[k % TANDEM_LENGTHS.size()]
		shots.append([TANDEM_SHOTS[k % TANDEM_SHOTS.size()], t, minf(t + length, to)])
		t += length
		k += 1
	return shots


# Frames for the Harrow screenshots: `[frame, shot]` where the follower's
# slide is largest with the lead close ahead.
func _tandem_stills(lead: Array, from: float, length: float) -> Array:
	var scored: Array = []
	var n := int(length * FPS)
	for k in range(2, n - 2, 6):
		var a: Dictionary = sim.replay_frame(from + float(k - 2) / FPS)
		var b: Dictionary = sim.replay_frame(from + float(k + 2) / FPS)
		if a.is_empty() or b.is_empty() or k >= lead.size() or lead[k].is_empty():
			continue
		var move: Vector3 = b.transform.origin - a.transform.origin
		move.y = 0.0
		var nose: Vector3 = -b.transform.basis.z
		nose.y = 0.0
		if move.length() < 0.1:
			continue
		var slip := rad_to_deg(move.angle_to(nose))
		var gap: float = lead[k].transform.origin.distance_to(b.transform.origin)
		if gap < TANDEM_CLOSE_M and gap > 6.0:
			scored.append([slip, k])
	scored.sort_custom(func(x, y): return x[0] > y[0])
	var picked: Array = []
	for e in scored:
		if picked.size() == HARROW_STILL_SHOTS.size():
			break
		var far := true
		for p in picked:
			if absi(p[0] - e[1]) < int(STILL_GAP_S * FPS):
				far = false
		if far:
			picked.append([e[1], HARROW_STILL_SHOTS[picked.size()]])
	return picked


# Places the camera for a shot: the replay director's, or one of the two
# that keep both tandem cars in frame.
func _camera(view, kind: String, xform: Transform3D, lead_view, shot_t: float) -> void:
	if not kind.begins_with("tandem") or lead_view == null:
		view._shot_camera("chase_low" if kind.begins_with("tandem") else kind, xform, shot_t)
		return
	var follow := xform.origin
	var ahead: Vector3 = lead_view.global_position
	var mid := (follow + ahead) * 0.5 + Vector3.UP * 0.6
	var dir := ahead - follow
	dir.y = 0.0
	var gap := dir.length()
	dir = dir.normalized() if gap > 0.1 else -xform.basis.z
	var side := dir.cross(Vector3.UP).normalized()
	camera.attributes = null
	if kind == "tandem":
		# Low behind the follower, looking up the road past it at the lead.
		camera.global_position = follow - dir * 7.0 + side * 2.0 + Vector3.UP * 1.6
		camera.look_at(mid + dir * 2.0, Vector3.UP)
		camera.fov = 50.0
	else:
		# High and to the side, both cars in the frame.
		camera.global_position = mid - dir * (0.4 * gap + 8.0) + side * (0.5 * gap + 10.0) + Vector3.UP * (0.3 * gap + 9.0)
		camera.look_at(mid, Vector3.UP)
		camera.fov = 45.0


# Places the cars at `t` (frame `k` of the segment); the replay car's
# transform, for the camera.
func _pose(view, lead_view, lead: Array, k: int, t: float) -> Transform3D:
	var frame: Dictionary = sim.replay_frame(t)
	if frame.is_empty():
		return view.car_view.global_transform
	view.time = t
	view.car_view.set_seat_view(false)
	view.car_view.update(frame.transform, frame.wheels, frame.steering_wheel_angle)
	view.world.update_cones(sim.replay_cones(t))
	if lead_view and k < lead.size() and not lead[k].is_empty():
		lead_view.update(lead[k].transform, lead[k].wheels, lead[k].steering_wheel_angle)
	return frame.transform
