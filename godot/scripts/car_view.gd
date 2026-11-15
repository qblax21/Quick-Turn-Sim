# SPDX-License-Identifier: Apache-2.0
# Placeholder car: a box body, a cabin, lamps and four wheels, sized from the
# dimensions the simulation core reports, plus a bare interior (dashboard,
# steering wheel, screen pillars) for the driver's-seat view. It is moved to
# the pose the core publishes each frame.
extends Node3D

var wheels: Array = []
var cabin_node: Node3D
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
	var mid_forward := cg_to_front - wheelbase * 0.5
	var bottom := -(cg_height - clearance)
	var body_color := Color(0.15, 0.45, 0.85) if electric else Color(0.85, 0.2, 0.15)
	var paint := _material(body_color, 0.35, 0.3)
	var dark := _material(Color(0.05, 0.05, 0.06), 0.8)

	# Narrower than the track so the wheels stay visible.
	var tire_width_front: float = info.get("tire_width_front", 0.22)
	var body_width := width - 2.0 * tire_width_front - 0.08
	_box(self, Vector3(body_width, height * 0.5, length), Vector3(0.0, bottom + height * 0.25, -mid_forward), paint)
	var nose := -mid_forward - length * 0.5
	var tail := -mid_forward + length * 0.5
	var lamp_y := bottom + height * 0.38
	for side in [-1.0, 1.0]:
		_box(self, Vector3(0.28, 0.09, 0.04), Vector3(side * (body_width * 0.5 - 0.2), lamp_y, nose), _material(Color(0.95, 0.95, 0.85), 0.2))
		_box(self, Vector3(0.28, 0.09, 0.04), Vector3(side * (body_width * 0.5 - 0.2), lamp_y, tail), _material(Color(0.7, 0.05, 0.05), 0.2))

	# Cabin set toward the rear so front and back are easy to tell apart.
	cabin_node = _box(self, Vector3(width * 0.66, height * 0.42, length * 0.5), Vector3(0.0, bottom + height * 0.71, -mid_forward + length * 0.1), _material(Color(0.10, 0.13, 0.18), 0.15, 0.4))

	# Eye point of a left-hand-drive seat: 55 percent of the wheelbase behind
	# the front axle, near roof height.
	var eye_height := clearance + 0.82 * height
	seat_offset = Vector3(-0.36, eye_height - cg_height, 0.55 * wheelbase - cg_to_front)
	var eye := seat_offset

	interior = Node3D.new()
	interior.visible = false
	add_child(interior)
	_box(interior, Vector3(width - 0.1, 0.12, 0.5), Vector3(0.0, eye.y - 0.32, eye.z - 0.82), dark)
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

	for i in range(4):
		var front := i < 2
		var radius: float = info.get("wheel_radius_front" if front else "wheel_radius_rear", 0.31)
		var tire_width: float = info.get("tire_width_front" if front else "tire_width_rear", 0.22)
		var pivot := Node3D.new()
		add_child(pivot)
		var tire := MeshInstance3D.new()
		var tire_mesh := CylinderMesh.new()
		tire_mesh.top_radius = radius
		tire_mesh.bottom_radius = radius
		tire_mesh.height = tire_width
		tire.mesh = tire_mesh
		tire.material_override = _material(Color(0.07, 0.07, 0.08), 0.9)
		# A cylinder's axis is local Y; the axle is local X.
		tire.rotation.z = PI * 0.5
		pivot.add_child(tire)
		# Light blocks on the wheel face so rotation is visible.
		var rim_material := _material(Color(0.75, 0.75, 0.78), 0.3, 0.6)
		_box(pivot, Vector3(tire_width * 1.04, radius * 1.3, radius * 0.22), Vector3.ZERO, rim_material)
		_box(pivot, Vector3(tire_width * 1.04, radius * 0.22, radius * 1.3), Vector3.ZERO, rim_material)
		wheels.append(pivot)


# In the driver's-seat view the cabin box is hidden, so it does not block the
# view, and the interior is shown.
func set_seat_view(on: bool) -> void:
	if cabin_node:
		cabin_node.visible = not on
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
