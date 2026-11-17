# SPDX-License-Identifier: Apache-2.0
# The car as drawn: a generated low-poly body (see car_body.gd) with glass,
# lamps, mirrors and spoked wheels, sized from the dimensions the simulation
# core reports, plus a bare interior (dashboard, steering wheel, screen
# pillars) for the driver's-seat view. It is moved to the pose the core
# publishes each frame.
extends Node3D

const CarBody := preload("res://scripts/car_body.gd")

var wheels: Array = []
var interior: Node3D
var steering_rim: Node3D
# Eye point of the driver's seat, relative to the centre of gravity.
var seat_offset := Vector3(-0.36, 0.6, 0.3)


func _material(color: Color, roughness: float = 0.6, metallic: float = 0.0) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.albedo_color = color
	m.roughness = roughness
	m.metallic = metallic
	return m


func _box(parent: Node3D, size: Vector3, position: Vector3, material: Material) -> MeshInstance3D:
	var node := MeshInstance3D.new()
	var mesh := BoxMesh.new()
	mesh.size = size
	node.mesh = mesh
	node.material_override = material
	node.position = position
	parent.add_child(node)
	return node


# A square bar from `a` to `b`.
func _bar(parent: Node3D, a: Vector3, b: Vector3, thickness: float, material: Material) -> void:
	var node := _box(parent, Vector3(thickness, thickness, a.distance_to(b)), (a + b) * 0.5, material)
	node.basis = Basis.looking_at(b - a, Vector3.UP)


func build(info: Dictionary) -> void:
	var length: float = info.get("length", 4.2)
	var width: float = info.get("width", 1.8)
	var height: float = info.get("height", 1.25)
	var clearance: float = info.get("ground_clearance", 0.13)
	var cg_height: float = info.get("cg_height", 0.5)
	var wheelbase: float = info.get("wheelbase", 2.6)
	var cg_to_front: float = info.get("cg_to_front_axle", 1.2)
	var electric: bool = info.get("electric", false)
	# The node's origin is the sprung centre of gravity. Forward is -Z.
	var body_color := Color(0.10, 0.36, 0.78) if electric else Color(0.80, 0.13, 0.10)
	var paint := _material(body_color, 0.28, 0.45)
	var glass := _material(Color(0.05, 0.07, 0.10), 0.08, 0.6)
	var dark := _material(Color(0.05, 0.05, 0.06), 0.8)

	var shape := CarBody.new()
	shape.setup(info)
	var body := MeshInstance3D.new()
	body.mesh = shape.build_mesh()
	body.set_surface_override_material(0, paint)
	body.set_surface_override_material(1, glass)
	add_child(body)

	var nose: float = shape.nose_z
	var tail: float = nose + length
	var front := shape.section(nose + 0.02 * length)
	var rear := shape.section(tail - 0.02 * length)
	var lamp_white := _material(Color(0.96, 0.96, 0.88), 0.15)
	lamp_white.emission_enabled = true
	lamp_white.emission = Color(0.9, 0.9, 0.8)
	lamp_white.emission_energy_multiplier = 0.6
	var lamp_red := _material(Color(0.75, 0.04, 0.04), 0.15)
	lamp_red.emission_enabled = true
	lamp_red.emission = Color(0.8, 0.05, 0.05)
	lamp_red.emission_energy_multiplier = 0.5
	var screen_base := shape.section(nose + shape.keys[3][0] * length)
	for side in [-1.0, 1.0]:
		_box(self, Vector3(0.36, 0.08, 0.16), Vector3(side * (front[5].x - 0.20), front[5].y - 0.07, nose + 0.07), lamp_white)
		_box(self, Vector3(0.40, 0.08, 0.16), Vector3(side * (rear[5].x - 0.22), rear[5].y - 0.08, tail - 0.07), lamp_red)
		# Door mirror at the base of the windscreen.
		_box(self, Vector3(0.17, 0.10, 0.07), Vector3(side * (screen_base[5].x + 0.07), screen_base[5].y + 0.05, screen_base[5].z + 0.25), paint)
	# Intake below the nose, a plate at the back and dark sills.
	_box(self, Vector3(width * 0.55, 0.13, 0.06), Vector3(0.0, front[0].y + 0.16, nose + 0.02), dark)
	_box(self, Vector3(0.46, 0.11, 0.03), Vector3(0.0, rear[5].y - 0.26, tail - 0.01), _material(Color(0.92, 0.92, 0.9), 0.5))

	# Eye point of a left-hand-drive seat: 55 percent of the wheelbase behind
	# the front axle, near roof height.
	var eye_height := clearance + 0.82 * height
	seat_offset = Vector3(-0.36, eye_height - cg_height, 0.55 * wheelbase - cg_to_front)
	var eye := seat_offset

	interior = Node3D.new()
	interior.visible = false
	add_child(interior)
	_box(interior, Vector3(width - 0.1, 0.12, 0.5), Vector3(0.0, eye.y - 0.32, eye.z - 0.82), dark)
	# Scuttle: fills the space under the base of the windscreen, so the seat
	# view does not look down through the single-sided body.
	_box(interior, Vector3(width - 0.08, 0.5, 0.7), Vector3(0.0, screen_base[5].y - 0.235, screen_base[5].z + 0.2), dark)
	# Instrument hood in front of the driver.
	_box(interior, Vector3(0.36, 0.07, 0.2), Vector3(eye.x, eye.y - 0.23, eye.z - 0.72), dark)
	# Screen pillars sit at the full body width, well outboard of the eye.
	var half := width * 0.5 - 0.03
	var left_base := Vector3(-half, eye.y - 0.27, eye.z - 0.95)
	var left_top := Vector3(-half + 0.1, eye.y + 0.45, eye.z - 0.3)
	var right_base := Vector3(half, eye.y - 0.27, eye.z - 0.95)
	var right_top := Vector3(half - 0.1, eye.y + 0.45, eye.z - 0.3)
	_bar(interior, left_base, left_top, 0.05, dark)
	_bar(interior, right_base, right_top, 0.05, dark)

	# Steering wheel: a column pivot raked back, and a rim that turns in it.
	var column := Node3D.new()
	column.position = Vector3(eye.x, eye.y - 0.36, eye.z - 0.5)
	column.rotation.x = -0.38
	interior.add_child(column)
	steering_rim = Node3D.new()
	column.add_child(steering_rim)
	var rim := MeshInstance3D.new()
	var torus := TorusMesh.new()
	torus.inner_radius = 0.155
	torus.outer_radius = 0.19
	rim.mesh = torus
	rim.material_override = dark
	# A torus lies in the XZ plane; stand it up to face the driver.
	rim.rotation.x = PI * 0.5
	steering_rim.add_child(rim)
	var spoke := _material(Color(0.22, 0.22, 0.24), 0.6)
	_box(steering_rim, Vector3(0.33, 0.035, 0.02), Vector3.ZERO, spoke)
	_box(steering_rim, Vector3(0.035, 0.16, 0.02), Vector3(0.0, -0.085, 0.0), spoke)
	# Top-centre mark.
	_box(steering_rim, Vector3(0.02, 0.035, 0.03), Vector3(0.0, 0.172, 0.0), _material(Color(0.9, 0.75, 0.1)))

	var rubber := _material(Color(0.06, 0.06, 0.07), 0.95)
	var alloy := _material(Color(0.72, 0.73, 0.76), 0.3, 0.7)
	for i in range(4):
		var is_front := i < 2
		var radius: float = info.get("wheel_radius_front" if is_front else "wheel_radius_rear", 0.31)
		var tire_width: float = info.get("tire_width_front" if is_front else "tire_width_rear", 0.22)
		var pivot := Node3D.new()
		add_child(pivot)
		# A cylinder's axis is local Y; the axle is local X.
		_wheel_disc(pivot, radius, tire_width, rubber)
		_wheel_disc(pivot, radius * 0.66, tire_width * 1.02, dark)
		_wheel_disc(pivot, radius * 0.16, tire_width * 1.08, alloy)
		# Five spokes and a rim band, a little proud of the tire wall.
		for k in range(5):
			var holder := Node3D.new()
			holder.rotation.x = TAU * k / 5.0
			pivot.add_child(holder)
			_box(holder, Vector3(tire_width * 1.06, radius * 0.56, radius * 0.13), Vector3(0.0, radius * 0.36, 0.0), alloy)
		for k in range(10):
			var holder := Node3D.new()
			holder.rotation.x = TAU * k / 10.0
			pivot.add_child(holder)
			_box(holder, Vector3(tire_width * 1.05, radius * 0.07, radius * 0.44), Vector3(0.0, radius * 0.64, 0.0), alloy)
		wheels.append(pivot)


func _wheel_disc(parent: Node3D, radius: float, width: float, material: Material) -> void:
	var node := MeshInstance3D.new()
	var mesh := CylinderMesh.new()
	mesh.top_radius = radius
	mesh.bottom_radius = radius
	mesh.height = width
	mesh.radial_segments = 20
	mesh.rings = 1
	node.mesh = mesh
	node.material_override = material
	node.rotation.z = PI * 0.5
	parent.add_child(node)


# Turns this car into the best-lap ghost: one translucent material, no
# shadow, no interior.
func make_ghost() -> void:
	var material := StandardMaterial3D.new()
	material.albedo_color = Color(0.75, 0.9, 1.0, 0.32)
	material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	material.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	if interior:
		interior.queue_free()
		interior = null
		steering_rim = null
	for node in find_children("*", "MeshInstance3D", true, false):
		node.material_override = material
		node.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	visible = false


# The body is single-sided, so from the driver's seat it does not block the
# view; only the interior needs switching on.
func set_seat_view(on: bool) -> void:
	if interior:
		interior.visible = on


# `states` holds five numbers per wheel: local x, y, z, steer angle and
# spin angle (see SimBridge.wheel_states). `steering_wheel_angle` is in
# radians, positive to the left.
func update(car_xform: Transform3D, states: PackedFloat32Array, steering_wheel_angle: float = 0.0) -> void:
	global_transform = car_xform
	if steering_rim:
		steering_rim.rotation.z = steering_wheel_angle
	for i in range(mini(wheels.size(), states.size() / 5)):
		var o := i * 5
		var pivot: Node3D = wheels[i]
		pivot.position = Vector3(states[o], states[o + 1], states[o + 2])
		# Default Euler order is YXZ: steer about Y, then roll about X.
		pivot.rotation = Vector3(-states[o + 4], states[o + 3], 0.0)
