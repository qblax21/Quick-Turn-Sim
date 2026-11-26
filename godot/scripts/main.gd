# SPDX-License-Identifier: Apache-2.0
# Front-end entry point. Owns the bridge to the simulation core and switches
# between the menu and driving. Nothing here computes physics, timing or
# force feedback: it sends inputs and draws what the core reports.
extends Node3D

const Menu := preload("res://scripts/menu.gd")
const InputSetup := preload("res://scripts/input_setup.gd")
const InputReader := preload("res://scripts/input_reader.gd")
const World := preload("res://scripts/world.gd")
const PhotoMode := preload("res://scripts/photo_mode.gd")
const Wizard := preload("res://scripts/wizard.gd")
const CarView := preload("res://scripts/car_view.gd")
const Hud := preload("res://scripts/hud.gd")
const SkidMarks := preload("res://scripts/skid_marks.gd")
const TyreSmoke := preload("res://scripts/tyre_smoke.gd")
const Audio := preload("res://scripts/audio.gd")
const Settings := preload("res://scripts/settings.gd")
const SettingsScreen := preload("res://scripts/settings_screen.gd")
const ReplayPicker := preload("res://scripts/replay_picker.gd")
const ReplayView := preload("res://scripts/replay_view.gd")
const SteamKit := preload("res://scripts/steam_kit.gd")
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
var scenery_aim := Vector3.ZERO
var pause_menu
var results
# The last drive request, so a session can be restarted or driven again.
var last_drive: Array = []
var photo_mode
# Highest hot tire pressure per wheel this session, Pa absolute, for the garage.
var hot_pressure_peak: Array = [0.0, 0.0, 0.0, 0.0]
var cold_pressure_set: Array = [0.0, 0.0, 0.0, 0.0]
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
var opponent_view
var skid_marks
var tyre_smoke
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
# The self-test drives with fixed input: the real wheel and pedals stay off.
var self_testing := false


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
	reader.sim_bridge = sim
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
	# First run: no controls set up yet (D-085).
	if not settings.get_value("input/wizard_done") and not FileAccess.file_exists(InputReader.CONFIG_PATH):
		_show_wizard()
	if "--self-test" in OS.get_cmdline_args() or "--self-test" in OS.get_cmdline_user_args():
		_self_test.call_deferred()
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--render-bench="):
			_render_bench.call_deferred(arg.get_slice("=", 1))
		elif arg.begins_with("--steam-kit="):
			_steam_kit.call_deferred(arg.get_slice("=", 1))
		elif arg.begins_with("--memory-check"):
			_memory_check.call_deferred()
		elif arg.begins_with("--title-shot="):
			_title_shot.call_deferred(arg.get_slice("=", 1))
		elif arg.begins_with("--ffb-meter-shot="):
			_ffb_meter_shot.call_deferred(arg.get_slice("=", 1))


# The computer drives Ridgeline with the force-feedback meter open; a
# picture after 40 s, then quit (D-123: checks the meter's layout).
func _ffb_meter_shot(path: String) -> void:
	self_testing = true
	_start_driving("hatch_tern_gt", "ridgeline_circuit", false, false, "fanatec_csl_elite", "", false)
	sim.set_ai_driver("grip", 0.8)
	hud.toggle_ffb_meter()
	await get_tree().create_timer(40.0).timeout
	get_viewport().get_texture().get_image().save_png(path)
	print("ffb meter shot saved: ", path, " suggested gain ", sim.status().get("suggested_gain", -1.0))
	sim.stop_session()
	get_tree().quit()


# Saves a picture of the title screen and quits, for checking its layout
# without a person at the screen.
func _title_shot(path: String) -> void:
	for i in 30:
		await get_tree().process_frame
	get_viewport().get_texture().get_image().save_png(path)
	print("title shot saved: ", path)
	get_tree().quit()


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
	get_viewport().scaling_3d_scale = clampf(settings.get_value("display/render_scale"), 0.5, 1.0)
	Engine.max_fps = maxi(int(settings.get_value("display/max_fps")), 0)
	if tyre_smoke:
		tyre_smoke.set_quality(settings.get_value("display/smoke"))
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
	self_testing = true
	_start_driving(cars[0], tracks[0], true, true, presets[0], "", false)
	if not sim.is_running():
		printerr("self-test: session did not start: " + sim.last_error())
		get_tree().quit(1)
		return
	var began := Time.get_ticks_msec()
	fixed_input = {"steer": 0.0, "throttle": 0.5, "brake": 0.0, "clutch": 0.0, "handbrake": 0.0, "buttons": 0}
	# --self-test-seconds=N drives longer (the demo's memory check, D-115).
	var seconds := 3.0
	for arg in OS.get_cmdline_user_args() + OS.get_cmdline_args():
		if arg.begins_with("--self-test-seconds="):
			seconds = maxf(arg.get_slice("=", 1).to_float(), 3.0)
	while Time.get_ticks_msec() - began < seconds * 1000.0:
		await get_tree().process_frame
	var speed: float = sim.hud().get("speed", 0.0)
	var status: Dictionary = sim.status()
	print("self-test: %s on %s, %.1f m/s after %.0f s, physics %.0f Hz" % [cars[0], tracks[0], speed, seconds, status.get("tick_rate", 0.0)])
	sim.stop_session()
	get_tree().quit(0 if speed > 2.0 else 1)


# The render bench (D-117, D-122, D-125). Started with
#   -- --render-bench=Low|High|all [--bench-low-end] [--bench-seconds=N]
#      [--bench-3d-height=N]
# The computer drives the Kestrel RC round Harrow from a standing start,
# sliding through the judged zones, seen from the chase camera with vsync
# and the frame cap off. Frame times are recorded from 5 s in until the lap
# ends (or BENCH_MAX_S, or --bench-seconds). Every run of the command writes
# one results file to send back: beside the game executable in an exported
# build, in user://bench otherwise. --bench-low-end caps the 3D resolution at
# 1280x720 (D-125); started with --gpu-index 1 as well it uses a second GPU
# if there is one, and the file says which adapter actually ran.
const BENCH_MAX_S := 300
const LOW_END_HEIGHT := 720.0


func _render_bench(which: String) -> void:
	var low_end := "--bench-low-end" in OS.get_cmdline_user_args()
	var limit := float(BENCH_MAX_S)
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--bench-seconds="):
			limit = clampf(arg.get_slice("=", 1).to_float(), 10.0, BENCH_MAX_S)
	var presets: Array = ["Low", "High"] if which.to_lower() == "all" else [which]
	var kept: Dictionary = settings.values.duplicate()
	var results: Array = []
	for preset in presets:
		var r: Dictionary = await _bench_one(preset, low_end, limit)
		if r.is_empty():
			settings.values = kept
			settings.save()
			get_tree().quit(1)
			return
		results.append(r)
		settings.values = kept.duplicate()
	settings.values = kept
	settings.save()
	var path := _write_bench_results(results, low_end)
	print("render bench: results written to ", path)
	get_tree().quit(0)


func _bench_one(preset: String, low_end: bool, limit: float) -> Dictionary:
	settings.apply_graphics_preset(preset)
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	Engine.max_fps = 0
	self_testing = true
	# The computer drives a real drift lap of Harrow (D-122), with no assists,
	# as a player would have to.
	settings.values["menu/traction_control"] = false
	_start_driving("coupe_kestrel_rc", "harrow_drift_park", false, false, sim.list_ids("ffb")[0], "", false)
	if not sim.is_running():
		printerr("render bench: session did not start: " + sim.last_error())
		return {}
	var size := get_viewport().get_visible_rect().size
	if low_end and size.y > LOW_END_HEIGHT:
		# The 3D drawn at 1280x720 (or less), whatever the window.
		get_viewport().scaling_3d_scale = minf(settings.get_value("display/render_scale"), LOW_END_HEIGHT / size.y)
	# --bench-3d-height=N draws the 3D at exactly N lines, above the window's
	# own if need be: a 1080p cost measured on a smaller screen.
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--bench-3d-height="):
			get_viewport().scaling_3d_scale = clampf(arg.get_slice("=", 1).to_float() / size.y, 0.25, 2.0)
	cockpit_view = false
	sim.set_ai_driver("drift", 0.9)
	var rid := get_viewport().get_viewport_rid()
	RenderingServer.viewport_set_measure_render_time(rid, true)
	var frames := PackedFloat32Array()
	var gpu := PackedFloat32Array()
	var began := Time.get_ticks_usec()
	var last := began
	var shot := false
	var h0: Dictionary = sim.hud()
	var laps_before: int = int(h0.get("runs", 0)) + int(h0.get("invalid_runs", 0))
	var lap_done := false
	# Memory (D-128): the largest figures seen, sampled twice a second.
	var mem_peak := {"ws": 0.0, "video_local": 0.0, "godot_video": 0.0, "textures": 0.0, "buffers": 0.0}
	var next_mem := 0.0
	while Time.get_ticks_usec() - began < limit * 1_000_000 and not lap_done:
		await get_tree().process_frame
		var now := Time.get_ticks_usec()
		var t := (now - began) / 1.0e6
		if t >= next_mem:
			next_mem = t + 0.5
			_sample_memory(mem_peak)
		var h: Dictionary = sim.hud()
		lap_done = int(h.get("runs", 0)) + int(h.get("invalid_runs", 0)) > laps_before
		if t > 16.0 and not shot:
			shot = true
			DirAccess.make_dir_recursive_absolute("user://bench")
			get_viewport().get_texture().get_image().save_jpg("user://bench/render_%s.jpg" % preset.to_lower())
		if t > 5.0:
			frames.append((now - last) / 1000.0)
			gpu.append(RenderingServer.viewport_get_measured_render_time_gpu(rid))
		last = now
	var seconds := (Time.get_ticks_usec() - began) / 1.0e6
	var sorted := frames.duplicate()
	sorted.sort()
	var n := maxi(sorted.size(), 1)
	var total := 0.0
	for f in sorted:
		total += f
	var count := maxi(n / 100, 1)
	var worst_1 := 0.0
	for k in range(maxi(n - count, 0), sorted.size()):
		worst_1 += sorted[k]
	var gpu_total := 0.0
	for g in gpu:
		gpu_total += g
	var status: Dictionary = sim.status()
	var scale: float = get_viewport().scaling_3d_scale
	var r := {
		"preset": preset,
		"window": "%dx%d" % [size.x, size.y],
		"render_scale": scale,
		"render_3d": "%dx%d" % [roundi(size.x * scale), roundi(size.y * scale)],
		"smoke": settings.get_value("display/smoke"),
		"frames": sorted.size(),
		"mean_ms": total / n,
		"fps": 1000.0 * n / maxf(total, 1e-6),
		"low1_ms": worst_1 / count,
		"low1_fps": 1000.0 * count / maxf(worst_1, 1e-6),
		"worst_ms": sorted[sorted.size() - 1] if sorted.size() > 0 else 0.0,
		"gpu_ms": gpu_total / maxf(gpu.size(), 1),
		"tick_us": status.get("step_mean_us", 0.0),
		"tick_worst_us": status.get("step_worst_us", 0.0),
		"seconds": seconds,
		"lap_finished": lap_done,
	}
	_sample_memory(mem_peak)
	var pm: Dictionary = sim.process_memory()
	var mb := 1.0 / 1048576.0
	r["ram_mb"] = float(pm.get("working_set", 0)) * mb
	r["ram_peak_mb"] = mem_peak.ws * mb
	r["vram_godot_mb"] = mem_peak.godot_video * mb
	r["vram_textures_mb"] = mem_peak.textures * mb
	r["vram_buffers_mb"] = mem_peak.buffers * mb
	r["vram_driver_mb"] = mem_peak.video_local * mb
	# With a software renderer (no video memory of its own, as on a CI
	# runner) the video memory sits in the process's RAM: take it out so
	# the RAM figure means the same on every machine.
	r["software_gpu"] = mem_peak.video_local == 0.0
	r["ram_excl_video_mb"] = r.ram_peak_mb - (r.vram_godot_mb if r.software_gpu else 0.0)
	r["textures"] = _largest_textures(8)
	# Godot's own allocations (0 where the build does not track them).
	r["godot_static_mb"] = float(OS.get_static_memory_peak_usage()) * mb
	print("render bench %s memory: Godot's own allocations peak %.0f MB" % [preset, r.godot_static_mb])
	print("render bench %s memory: RAM peak %.0f MB (%.0f MB without video memory), video memory %.0f MB by Godot's count (textures %.0f, buffers %.0f), %.0f MB by the driver's" % [
		preset, r.ram_peak_mb, r.ram_excl_video_mb, r.vram_godot_mb, r.vram_textures_mb, r.vram_buffers_mb, r.vram_driver_mb])
	print("render bench %s: %d frames, 3D at %s, mean %.2f ms (%.0f fps), 1%% low %.2f ms (%.0f fps), worst %.2f ms, GPU %.2f ms, physics tick %.0f us, %s after %.0f s" % [
		preset, r.frames, r.render_3d, r.mean_ms, r.fps, r.low1_ms, r.low1_fps, r.worst_ms, r.gpu_ms, r.tick_us,
		"lap finished" if lap_done else "lap not finished", seconds])
	sim.stop_session()
	return r


# Largest figures so far into `peak`: the process working set and the
# driver's video memory from the OS, Godot's own video memory count.
func _sample_memory(peak: Dictionary) -> void:
	var pm: Dictionary = sim.process_memory()
	peak.ws = maxf(peak.ws, float(pm.get("peak_working_set", 0)))
	peak.video_local = maxf(peak.video_local, float(pm.get("video_local", 0)))
	peak.godot_video = maxf(peak.godot_video, float(RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_VIDEO_MEM_USED)))
	peak.textures = maxf(peak.textures, float(RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TEXTURE_MEM_USED)))
	peak.buffers = maxf(peak.buffers, float(RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_BUFFER_MEM_USED)))


# The `count` largest textures the scene uses, as "WxH, N MB, where":
# from every material, sky and particle in the tree, at 4 bytes a pixel
# plus a third for mipmaps (an estimate: compressed formats take less).
# Godot 4 offers scripts no list of the textures the renderer holds, so
# render targets and shadow maps are not in it.
func _largest_textures(count: int) -> Array:
	var seen := {}
	var stack: Array = [get_tree().root]
	while not stack.is_empty():
		var node: Node = stack.pop_back()
		stack.append_array(node.get_children())
		var mats: Array = []
		if node is GeometryInstance3D and node.material_override:
			mats.append(node.material_override)
		if node is MeshInstance3D and node.mesh:
			for i in node.mesh.get_surface_count():
				mats.append(node.get_active_material(i))
		if node is WorldEnvironment and node.environment and node.environment.sky:
			mats.append(node.environment.sky.sky_material)
		for m in mats:
			if m == null:
				continue
			for prop in m.get_property_list():
				var v = m.get(prop.name)
				if v is Texture2D and not seen.has(v):
					seen[v] = "%s.%s" % [m.resource_path.get_file() if m.resource_path != "" else m.get_class(), prop.name]
	var list: Array = []
	for t in seen:
		var bytes := float(t.get_width() * t.get_height() * 4) * 4.0 / 3.0
		list.append([bytes, "%dx%d, %.1f MB, %s" % [t.get_width(), t.get_height(), bytes / 1048576.0, seen[t]]])
	list.sort_custom(func(a, b): return a[0] > b[0])
	var out: Array = []
	for e in list.slice(0, count):
		out.append(e[1])
	return out


# The store page kit (D-129): screenshots and trailer frames from replays;
# see steam_kit.gd and tools\steam_kit.ps1.
func _steam_kit(out: String) -> void:
	_clear_session()
	_close_overlays()
	_hide_scenery()
	self_testing = true
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	var kit = SteamKit.new()
	add_child(kit)
	var code: int = await kit.run(sim, camera, out)
	get_tree().quit(code)


# The memory budget check (D-128), for CI: Harrow on High, the computer
# driving, for --bench-seconds (default 60). Prints one line for
# tools\memory_check.ps1 to compare with tools\memory_budget.txt, and exits.
func _memory_check() -> void:
	var limit := 60.0
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--bench-seconds="):
			limit = clampf(arg.get_slice("=", 1).to_float(), 10.0, BENCH_MAX_S)
	var kept: Dictionary = settings.values.duplicate()
	var r: Dictionary = await _bench_one("High", false, limit)
	settings.values = kept
	if r.is_empty():
		get_tree().quit(1)
		return
	print("memory check: ram_mb=%.0f ram_excl_video_mb=%.0f vram_godot_mb=%.0f vram_driver_mb=%.0f software_gpu=%s render_3d=%s frames=%d" % [
		r.ram_peak_mb, r.ram_excl_video_mb, r.vram_godot_mb, r.vram_driver_mb, r.software_gpu, r.render_3d, r.frames])
	for line in r.textures:
		print("memory check texture: ", line)
	get_tree().quit(0)


# One plain-text file a tester can send back: the machine, then one block
# per preset.
func _write_bench_results(results: Array, low_end: bool) -> String:
	var stamp := Time.get_datetime_string_from_system().replace(":", "-")
	var dir := "user://bench"
	if OS.has_feature("template"):
		dir = OS.get_executable_path().get_base_dir()
	DirAccess.make_dir_recursive_absolute(dir)
	var path := dir.path_join("render_bench_%s.txt" % stamp)
	var mem: Dictionary = OS.get_memory_info()
	var lines: Array = [
		"%s render bench, version %s" % [sim.product_name(), sim.version()],
		"date        %s" % Time.get_datetime_string_from_system(),
		"OS          %s %s" % [OS.get_name(), OS.get_version()],
		"CPU         %s, %d threads" % [OS.get_processor_name(), OS.get_processor_count()],
		"RAM         %.1f GB" % (float(mem.get("physical", 0)) / 1073741824.0),
		"GPU         %s (%s)" % [RenderingServer.get_video_adapter_name(), RenderingServer.get_video_adapter_vendor()],
		"driver      %s" % ", ".join(OS.get_video_adapter_driver_info()),
		"API         %s %s" % [RenderingServer.get_current_rendering_driver_name(), RenderingServer.get_video_adapter_api_version()],
		"mode        %s" % ("low-end (3D capped at 1280x720)" if low_end else "normal"),
		"run         the computer drives Harrow in the Kestrel RC from a standing start; frames from 5 s in",
		"",
	]
	for r in results:
		lines.append("[%s]" % r.preset)
		lines.append("window      %s, 3D drawn at %s (render scale %.2f), smoke %d" % [r.window, r.render_3d, r.render_scale, r.smoke])
		lines.append("frames      %d over %.0f s, %s" % [r.frames, r.seconds, "lap finished" if r.lap_finished else "lap not finished"])
		lines.append("frame time  mean %.2f ms (%.0f fps), 1%% low %.2f ms (%.0f fps), worst %.2f ms" % [r.mean_ms, r.fps, r.low1_ms, r.low1_fps, r.worst_ms])
		lines.append("GPU time    mean %.2f ms per frame" % r.gpu_ms)
		lines.append("physics     tick mean %.0f us, worst %.0f us (budget 1000 us)" % [r.tick_us, r.tick_worst_us])
		lines.append("RAM         working set peak %.0f MB, %.0f MB at the end; Godot's own allocations peak %.0f MB" % [r.ram_peak_mb, r.ram_mb, r.godot_static_mb])
		lines.append("video mem   %.0f MB by Godot's count (textures %.0f, buffers %.0f), %.0f MB by the driver's%s" % [
			r.vram_godot_mb, r.vram_textures_mb, r.vram_buffers_mb, r.vram_driver_mb,
			" (software renderer: in RAM)" if r.software_gpu else ""])
		for t in r.textures:
			lines.append("  texture   %s" % t)
		lines.append("")
	var f := FileAccess.open(path, FileAccess.WRITE)
	if f:
		f.store_string("
".join(lines))
		f.close()
	return ProjectSettings.globalize_path(path)


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
	scenery_aim = Vector3.ZERO


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
	var at := _scenery_point(scenery_s)
	var ahead := _scenery_point(scenery_s + 40.0)
	var along := (ahead - at)
	along.y = 0.0
	along = along.normalized() if along.length() > 0.01 else Vector3.FORWARD
	var side := Vector3(along.z, 0.0, -along.x)
	# Height from the road averaged over a long stretch, so the camera does
	# not ride every bump and kerb of the track's profile.
	var height := 0.0
	for k in range(-30, 31, 3):
		height += _scenery_point(scenery_s + k).y
	height /= 21.0
	var goal := Vector3(at.x, height, at.z) + side * 25.0 + Vector3.UP * 22.0
	var aim := Vector3(ahead.x, height + 2.0, ahead.z)
	# Ease toward the goal rather than snapping to it: a steady drone shot.
	var blend := 1.0 - exp(-1.5 * delta)
	if scenery_aim == Vector3.ZERO:
		camera.global_position = goal
		scenery_aim = aim
	camera.global_position = camera.global_position.lerp(goal, blend)
	scenery_aim = scenery_aim.lerp(aim, blend)
	camera.look_at(scenery_aim, Vector3.UP)
	camera.fov = 60.0


# A point on the scenery path at a fractional index, interpolated and
# wrapped round the loop.
func _scenery_point(s_index: float) -> Vector3:
	var n := scenery_points.size()
	var s_wrapped := fposmod(s_index, float(n))
	# Rounding can give exactly n: wrap the index as well.
	var i := int(s_wrapped) % n
	return scenery_points[i].lerp(scenery_points[(i + 1) % n], clampf(s_wrapped - int(s_wrapped), 0.0, 1.0))


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


func _show_wizard() -> void:
	var wizard = Wizard.new()
	wizard.setup(sim, reader, settings)
	wizard.closed.connect(func() -> void: wizard.queue_free())
	add_child(wizard)


func _show_input_setup() -> void:
	_close_overlays()
	setup = InputSetup.new()
	setup.setup(sim, reader, settings)
	setup.wizard_requested.connect(func() -> void:
		setup.queue_free()
		setup = null
		_show_menu()
		_show_wizard())
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
	for node in [world, car_view, ghost_view, opponent_view, skid_marks, tyre_smoke, hud]:
		if node:
			node.queue_free()
	world = null
	car_view = null
	ghost_view = null
	opponent_view = null
	skid_marks = null
	tyre_smoke = null
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
	if sim.has_method("set_match_rotation"):
		sim.set_match_rotation(settings.get_value("ffb/match_rotation"))
	if sim.has_method("set_engine_damage"):
		sim.set_engine_damage(settings.get_value("menu/engine_damage"))
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
	if settings.get_value("driving/lap_telemetry"):
		var stamp := Time.get_datetime_string_from_system().replace(":", "-")
		var lap_dir := ProjectSettings.globalize_path("user://recordings/laps").path_join("%s_%s_%s" % [car_id, track_id, stamp])
		sim.set_lap_telemetry(lap_dir)
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
	_add_computer_car(info)
	audio.start(info, sim.track_info())
	skid_marks = SkidMarks.new()
	add_child(skid_marks)
	skid_marks.setup(info)
	tyre_smoke = TyreSmoke.new()
	add_child(tyre_smoke)
	tyre_smoke.setup(info, settings.get_value("display/smoke"))
	hud = Hud.new()
	add_child(hud)
	hud.setup(sim)
	hud.ffb_meter.gain_requested.connect(_apply_suggested_gain)
	hud.set_map_mode(map_mode)
	if device_id != "":
		sim.ffb_open(device_id, DisplayServer.window_get_native_handle(DisplayServer.WINDOW_HANDLE))
		sim.ffb_set_direct_steering(direct_steering)
	# Wheel, pedals and shifter read on the physics thread at full rate.
	var direct_map: Dictionary = reader.direct_map()
	if not self_testing and (not direct_map.axes.is_empty() or not direct_map.buttons.is_empty()):
		if not sim.set_direct_inputs(direct_map):
			hud.flash(sim.last_error())
	_check_shifter(auto_clutch)


# An H-pattern shifter that cannot work as set up says so at the start,
# instead of silently grinding or doing nothing (D-110).
func _check_shifter(auto_clutch: bool) -> void:
	var any_gate := false
	for gate in reader.GEAR_GATES:
		any_gate = any_gate or reader.has_direct(gate) or reader.buttons[gate].device >= 0
	if reader.shift_mode != "h_pattern":
		if any_gate:
			hud.flash("Shifter gates are assigned but Gear change is Sequential, so the lever is ignored (Controls).", 8.0)
		return
	var has_clutch: bool = reader.has_direct("clutch") or reader.axes["clutch"].device >= 0
	if not any_gate:
		hud.flash("H-pattern is selected but no shifter gate is assigned (Controls).", 8.0)
	elif not auto_clutch and not has_clutch:
		hud.flash("No clutch pedal assigned and Automatic clutch is off: gears will not go in.", 8.0)


func _process(delta: float) -> void:
	if scenery and not sim.is_running():
		_fly_scenery(delta)
	if sim == null or not sim.is_running():
		return
	_track_pressures()
	var input: Dictionary = fixed_input if not fixed_input.is_empty() else reader.read(delta)
	sim.set_input(input.steer, input.throttle, input.brake, input.clutch, input.handbrake, input.buttons, input.get("gear", -128))
	var car_xform: Transform3D = sim.car_transform()
	var values: Dictionary = sim.hud()
	_note_laps(values)
	car_view.update(car_xform, sim.wheel_states(), values.get("steering_wheel_angle", 0.0))
	world.update_cones(sim.cones_down())
	skid_marks.update(car_xform, sim.wheel_states(), values)
	tyre_smoke.update(car_xform, sim.wheel_states(), values)
	var ghost: Dictionary = sim.ghost()
	values["ghost_time"] = ghost.get("time", 0.0)
	values["ghost_setup"] = ghost.get("setup", "")
	values["ghost_score"] = ghost.get("score", -1.0)
	values["ghost_has_delta"] = ghost.get("has_delta", false)
	values["ghost_delta"] = ghost.get("delta", 0.0)
	_update_ghost(ghost, car_xform)
	if opponent_view:
		var opp: Dictionary = sim.opponent()
		opponent_view.visible = opp.get("visible", false)
		if opponent_view.visible:
			opponent_view.update(opp.transform, opp.wheels)
	hud.update_map(car_xform, ghost_view.visible, ghost_view.global_transform)
	hud.update(values, sim.status(), input, telemetry_on, replay_on)
	audio.update(values, input, delta, paused)
	_update_camera(car_xform, delta)


# The computer car (D-122): the same model as yours, driven through the
# same inputs by the core's AI driver. Drift mode where the track has judged
# zones, grip laps elsewhere.
func _add_computer_car(info: Dictionary) -> void:
	var choice: int = settings.get_value("menu/computer_car")
	if choice == 0 or self_testing:
		return
	var ahead := 30.0 if choice == 2 else 0.0
	if not sim.set_opponent(session_car, "drift", 0.85, ahead):
		if not sim.set_opponent(session_car, "grip", 0.85, ahead):
			push_warning("computer car: " + sim.last_error())
			return
	opponent_view = CarView.new()
	add_child(opponent_view)
	opponent_view.build(info)
	opponent_view.visible = false


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
	if not event.pressed or event.echo or sim == null or not sim.is_running() or photo_mode:
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
		KEY_F6:
			hud.toggle_perf_overlay()
		KEY_F7:
			hud.toggle_coach()
		KEY_F9:
			hud.toggle_drift_panel()
		KEY_F8:
			_open_photo_mode()
		KEY_F5:
			var state: String = sim.status().get("ffb_log_state", "off")
			if state == "waiting" or state == "recording":
				sim.ffb_log_stop()
			else:
				sim.ffb_log_start(_recording_path("ffb_" + session_car + "_" + session_track, "csv"))
				hud.flash("FFB log armed: it records the next full lap")
		KEY_K:
			_apply_suggested_gain()
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


# The gain the core suggests for clipping under 2 % of this lap (D-123).
func _apply_suggested_gain() -> void:
	if not hud.ffb_meter_visible():
		return
	var g: float = sim.status().get("suggested_gain", -1.0)
	if g <= 0.0:
		hud.flash("No gain suggestion yet: drive 5 s of a timed lap")
		return
	var config: Dictionary = sim.ffb_config()
	if sim.ffb_set("gain", snappedf(g, 0.01)):
		settings.set_ffb(config.get("id", ""), "gain", snappedf(g, 0.01))
		settings.save()
		hud.flash("gain %.2f, for clipping under 2%% of a lap like this one" % g)


# Photo mode: the session is paused while it is open and resumes after,
# unless it was paused already.
func _open_photo_mode() -> void:
	if photo_mode:
		return
	var was_paused := paused
	sim.set_paused(true)
	var hud_was_visible: bool = hud.visible
	hud.visible = false
	photo_mode = PhotoMode.new()
	photo_mode.setup(camera)
	photo_mode.closed.connect(func() -> void:
		photo_mode = null
		hud.visible = hud_was_visible
		if not was_paused:
			sim.set_paused(false))
	add_child(photo_mode)


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


# Remembers the hottest tire pressures of the session.
func _track_pressures() -> void:
	var hud: Dictionary = sim.hud()
	if not hud.has("pressure"):
		return
	for i in 4:
		hot_pressure_peak[i] = maxf(hot_pressure_peak[i], hud.pressure[i])
		cold_pressure_set[i] = hud.get("pressure_cold", hud.pressure)[i]


# Leaves the session and shows what happened in it.
func _end_session() -> void:
	if last_drive.size() > 1 and hot_pressure_peak[0] > 0.0:
		settings.set_value("garage/pressures/" + last_drive[0], {
			"track": sim.track_info().get("name", ""),
			"cold": cold_pressure_set.duplicate(), "hot": hot_pressure_peak.duplicate()})
		settings.save()
	hot_pressure_peak = [0.0, 0.0, 0.0, 0.0]
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
