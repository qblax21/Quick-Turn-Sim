# SPDX-License-Identifier: Apache-2.0
# Placeholder sound, synthesised at start-up: no recorded samples. Each sound
# is a short loop whose pitch and volume follow values the simulation core
# reports (engine speed, slip, load, speed). Presentation only: nothing here
# feeds back into the simulation.
extends Node

const RATE := 22050
const SMOOTHING_PER_S := 14.0
# Engine loop is built for this crank speed; pitch scales from it.
const ENGINE_REF_RPM := 3000.0
const MOTOR_REF_RPM := 6000.0
const LOOSE_SURFACES := ["gravel", "dirt"]

var players := {}
var levels := {}
var electric := false
var loose := PackedByteArray()
var active := false
var muted := false


func _ready() -> void:
	var rng := RandomNumberGenerator.new()
	rng.seed = 20261115
	_add("engine_on", _harmonics({0.5: 0.15, 1.0: 0.25, 1.5: 0.12, 2.0: 1.0, 2.5: 0.1, 3.0: 0.2, 4.0: 0.6, 5.0: 0.1, 6.0: 0.4, 8.0: 0.25, 10.0: 0.15, 12.0: 0.1}, ENGINE_REF_RPM / 60.0, rng))
	_add("engine_off", _harmonics({1.0: 0.2, 2.0: 1.0, 4.0: 0.35, 6.0: 0.12}, ENGINE_REF_RPM / 60.0, rng))
	_add("motor", _harmonics({6.0: 1.0, 12.0: 0.3, 18.0: 0.15, 24.0: 0.06}, MOTOR_REF_RPM / 60.0, rng))
	_add("squeal", _noise(rng, 2.0, [[900.0, 8.0, 1.0], [1400.0, 8.0, 0.6]], false))
	_add("loose", _noise(rng, 2.0, [[500.0, 0.6, 1.0], [1800.0, 0.8, 0.5]], true))
	_add("road", _noise(rng, 2.0, [[140.0, 0.7, 1.0]], false))
	_add("wind", _noise(rng, 2.0, [[1500.0, 0.5, 1.0]], false))


func _add(key: String, samples: PackedFloat32Array) -> void:
	var peak := 0.0001
	for v in samples:
		peak = maxf(peak, absf(v))
	var bytes := PackedByteArray()
	bytes.resize(samples.size() * 2)
	for i in range(samples.size()):
		bytes.encode_s16(i * 2, int(samples[i] / peak * 0.9 * 32767.0))
	var stream := AudioStreamWAV.new()
	stream.format = AudioStreamWAV.FORMAT_16_BITS
	stream.mix_rate = RATE
	stream.stereo = false
	stream.data = bytes
	stream.loop_mode = AudioStreamWAV.LOOP_FORWARD
	stream.loop_begin = 0
	stream.loop_end = samples.size()
	var player := AudioStreamPlayer.new()
	player.stream = stream
	player.volume_db = -80.0
	add_child(player)
	players[key] = player
	levels[key] = 0.0


# One second of a sum of sines. `orders` maps multiples of `base_hz` to
# amplitudes; base_hz * order must be a whole number so the loop is seamless.
func _harmonics(orders: Dictionary, base_hz: float, rng: RandomNumberGenerator) -> PackedFloat32Array:
	var out := PackedFloat32Array()
	out.resize(RATE)
	for order in orders:
		var step: float = TAU * round(base_hz * order) / RATE
		var amp: float = orders[order]
		var phase := rng.randf() * TAU
		for i in range(RATE):
			out[i] += amp * sin(phase + step * i)
	return out


# Filtered noise. Each band is [centre Hz, Q, gain]. `crunch` adds a slow
# random envelope, for loose surfaces.
func _noise(rng: RandomNumberGenerator, seconds: float, bands: Array, crunch: bool) -> PackedFloat32Array:
	var n := int(seconds * RATE)
	var fade := RATE / 8
	var total := n + fade
	var white := PackedFloat32Array()
	white.resize(total)
	var envelope := 1.0
	var target := 1.0
	for i in range(total):
		if crunch:
			if i % 180 == 0:
				target = rng.randf_range(0.25, 1.0)
			envelope += (target - envelope) * 0.02
		white[i] = rng.randf_range(-1.0, 1.0) * envelope
	var mixed := PackedFloat32Array()
	mixed.resize(total)
	for band in bands:
		var w0: float = TAU * band[0] / RATE
		var alpha: float = sin(w0) / (2.0 * band[1])
		var a0 := 1.0 + alpha
		var b0 := alpha / a0
		var a1 := -2.0 * cos(w0) / a0
		var a2 := (1.0 - alpha) / a0
		var gain: float = band[2]
		var x1 := 0.0
		var x2 := 0.0
		var y1 := 0.0
		var y2 := 0.0
		for i in range(total):
			var x := white[i]
			var y := b0 * (x - x2) - a1 * y1 - a2 * y2
			x2 = x1
			x1 = x
			y2 = y1
			y1 = y
			mixed[i] += gain * y
	# Blend the tail into the head so the loop has no click.
	var out := PackedFloat32Array()
	out.resize(n)
	for i in range(n):
		out[i] = mixed[i]
	for i in range(fade):
		var t := float(i) / fade
		out[i] = mixed[i] * t + mixed[n + i] * (1.0 - t)
	return out


func start(car_info: Dictionary, track_info: Dictionary) -> void:
	electric = car_info.get("electric", false)
	loose = PackedByteArray()
	for id in track_info.get("surface_ids", PackedStringArray()):
		loose.append(1 if id in LOOSE_SURFACES else 0)
	active = true
	for key in players:
		levels[key] = 0.0
		players[key].volume_db = -80.0
		players[key].play()


func stop() -> void:
	active = false
	for key in players:
		players[key].stop()


func toggle_mute() -> void:
	muted = not muted


func _smoothstep(lo: float, hi: float, x: float) -> float:
	var t := clampf((x - lo) / (hi - lo), 0.0, 1.0)
	return t * t * (3.0 - 2.0 * t)


func _drive(key: String, level: float, pitch: float, delta: float) -> void:
	var blend := 1.0 - exp(-SMOOTHING_PER_S * delta)
	levels[key] += (level - levels[key]) * blend
	var player: AudioStreamPlayer = players[key]
	player.volume_db = linear_to_db(maxf(levels[key], 0.0001))
	player.pitch_scale = clampf(pitch, 0.05, 4.0)


func update(hud: Dictionary, input: Dictionary, delta: float, paused: bool) -> void:
	if not active:
		return
	var master := 0.0 if (paused or muted) else 1.0
	var speed: float = absf(hud.get("speed", 0.0))
	var rpm: float = hud.get("engine_rpm", 0.0)
	var throttle: float = clampf(input.get("throttle", 0.0), 0.0, 1.0)

	if electric:
		var power: float = maxf(hud.get("drive_power_w", 0.0), hud.get("regen_power_w", 0.0))
		var load := clampf(power / 250000.0, 0.0, 1.0)
		var turning := clampf(rpm / 300.0, 0.0, 1.0)
		_drive("motor", master * turning * (0.04 + 0.3 * load), 0.2 + rpm / MOTOR_REF_RPM * 0.8, delta)
		_drive("engine_on", 0.0, 1.0, delta)
		_drive("engine_off", 0.0, 1.0, delta)
	else:
		var pitch := clampf(rpm, 500.0, 9000.0) / ENGINE_REF_RPM
		var running := clampf(rpm / 400.0, 0.0, 1.0)
		var rev := clampf(rpm / 7000.0, 0.0, 1.0)
		_drive("engine_on", master * running * throttle * (0.3 + 0.4 * rev), pitch, delta)
		_drive("engine_off", master * running * (1.0 - 0.7 * throttle) * (0.14 + 0.2 * rev), pitch, delta)
		_drive("motor", 0.0, 1.0, delta)

	# Tires: how hard each one is sliding, weighted by its load.
	var slip_ratio: PackedFloat32Array = hud.get("slip_ratio", PackedFloat32Array())
	var slip_angle: PackedFloat32Array = hud.get("slip_angle", PackedFloat32Array())
	var load_n: PackedFloat32Array = hud.get("load", PackedFloat32Array())
	var surface: PackedInt32Array = hud.get("surface", PackedInt32Array())
	var moving := clampf(speed / 3.0, 0.0, 1.0)
	var driven := 1.0 if (speed > 0.5 or throttle > 0.1) else 0.0
	var squeal := 0.0
	var crunch := 0.0
	var count := mini(mini(slip_ratio.size(), slip_angle.size()), mini(load_n.size(), surface.size()))
	for i in range(count):
		var weight := clampf(load_n[i] / 4000.0, 0.0, 1.5)
		var lateral := tan(clampf(slip_angle[i], -1.2, 1.2)) * moving
		var slide := sqrt(slip_ratio[i] * slip_ratio[i] * driven + lateral * lateral)
		var on_loose: bool = surface[i] >= 0 and surface[i] < loose.size() and loose[surface[i]] == 1
		if on_loose:
			crunch += weight * (0.35 * clampf(speed / 20.0, 0.0, 1.0) + _smoothstep(0.05, 0.4, slide))
		else:
			squeal += weight * _smoothstep(0.07, 0.30, slide)
	if count > 0:
		squeal /= count
		crunch /= count
	_drive("squeal", master * clampf(squeal, 0.0, 1.0) * 0.55, 0.85 + 0.3 * clampf(squeal, 0.0, 1.0), delta)
	_drive("loose", master * clampf(crunch, 0.0, 1.0) * 0.6, 0.8 + speed / 60.0, delta)
	_drive("road", master * clampf(speed / 40.0, 0.0, 1.0) * 0.3, 0.6 + speed / 50.0, delta)
	var wind := clampf(speed / 45.0, 0.0, 1.0)
	_drive("wind", master * wind * wind * 0.3, 0.7 + speed / 80.0, delta)
