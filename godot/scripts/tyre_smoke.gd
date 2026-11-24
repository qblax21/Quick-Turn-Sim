# SPDX-License-Identifier: Apache-2.0
# Tyre smoke (D-117): a particle emitter at each tyre's contact patch that
# smokes while the tyre slides or spins, by the same slip measure as the
# tyre marks. Drawing only: smoke does not change grip.
#
# Quality (Settings > Tyre smoke, or the graphics preset):
#   0  off
#   1  low   few short-lived puffs per wheel
#   2  high  dense, long-lived clouds
extends Node3D

const SkidMarks := preload("res://scripts/skid_marks.gd")
# Particles alive at once per wheel, and how long each lasts, by quality.
const AMOUNT := [0, 24, 240]
const LIFETIME_S := [0.0, 1.4, 4.0]
const LIFT_M := 0.15

var emitters: Array = []
var radius := [0.31, 0.31, 0.31, 0.31]
var quality := 2


func setup(info: Dictionary, smoke_quality: int) -> void:
	for i in range(4):
		radius[i] = info.get("wheel_radius_front" if i < 2 else "wheel_radius_rear", 0.31)
	var gradient := Gradient.new()
	gradient.set_color(0, Color(1, 1, 1, 1))
	gradient.set_color(1, Color(1, 1, 1, 0))
	var puff := GradientTexture2D.new()
	puff.gradient = gradient
	puff.fill = GradientTexture2D.FILL_RADIAL
	puff.fill_from = Vector2(0.5, 0.5)
	puff.fill_to = Vector2(0.5, 0.0)
	puff.width = 64
	puff.height = 64
	var draw := StandardMaterial3D.new()
	draw.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	draw.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	draw.billboard_mode = BaseMaterial3D.BILLBOARD_PARTICLES
	draw.vertex_color_use_as_albedo = true
	draw.albedo_texture = puff
	draw.depth_draw_mode = BaseMaterial3D.DEPTH_DRAW_DISABLED
	var quad := QuadMesh.new()
	quad.size = Vector2(1.2, 1.2)
	quad.material = draw
	var fade := Gradient.new()
	fade.set_color(0, Color(0.86, 0.86, 0.86, 0.3))
	fade.set_color(1, Color(0.9, 0.9, 0.9, 0.0))
	var ramp := GradientTexture1D.new()
	ramp.gradient = fade
	var grow := Curve.new()
	grow.add_point(Vector2(0.0, 0.35))
	grow.add_point(Vector2(1.0, 1.0))
	var grow_texture := CurveTexture.new()
	grow_texture.curve = grow
	for i in range(4):
		var p := GPUParticles3D.new()
		var process := ParticleProcessMaterial.new()
		process.direction = Vector3(0, 1, 0)
		process.spread = 70.0
		process.initial_velocity_min = 0.4
		process.initial_velocity_max = 1.6
		process.gravity = Vector3(0, 0.5, 0)
		process.damping_min = 0.6
		process.damping_max = 1.2
		process.scale_min = 1.0
		process.scale_max = 2.2
		process.scale_curve = grow_texture
		process.color_ramp = ramp
		p.process_material = process
		p.draw_pass_1 = quad
		p.local_coords = false
		p.emitting = false
		p.visibility_aabb = AABB(Vector3(-20, -2, -20), Vector3(40, 14, 40))
		add_child(p)
		emitters.append(p)
	set_quality(smoke_quality)


func set_quality(q: int) -> void:
	quality = clampi(q, 0, AMOUNT.size() - 1)
	for p in emitters:
		p.emitting = false
		if quality > 0:
			p.amount = AMOUNT[quality]
			p.lifetime = LIFETIME_S[quality]


func update(car_xform: Transform3D, states: PackedFloat32Array, hud: Dictionary) -> void:
	if quality == 0 or hud.is_empty() or states.size() < 20 or not hud.has("slip_ratio"):
		return
	for i in range(4):
		var o := i * 5
		var center := car_xform * Vector3(states[o], states[o + 1], states[o + 2])
		var p: GPUParticles3D = emitters[i]
		p.global_position = center - car_xform.basis.y * (radius[i] - LIFT_M)
		var level := SkidMarks.slide_level(hud, i)
		p.amount_ratio = clampf(level, 0.0, 1.0)
		p.emitting = level > 0.05
