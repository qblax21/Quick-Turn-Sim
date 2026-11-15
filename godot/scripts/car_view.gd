# SPDX-License-Identifier: Apache-2.0
# Placeholder car: a box body, a smaller cabin box and four cylinders,
# sized from the dimensions the simulation core reports. It is moved to the
# pose the core publishes each frame.
extends Node3D

var wheels: Array = []
var cabin_node: MeshInstance3D


func _material(color: Color) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.albedo_color = color
	m.roughness = 0.6
	return m


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

	var lower := MeshInstance3D.new()
	var lower_mesh := BoxMesh.new()
	# Narrower than the track so the wheels stay visible.
	var tire_width_front: float = info.get("tire_width_front", 0.22)
	lower_mesh.size = Vector3(width - 2.0 * tire_width_front - 0.08, height * 0.5, length)
	lower.mesh = lower_mesh
	lower.material_override = _material(body_color)
	lower.position = Vector3(0.0, bottom + height * 0.25, -mid_forward)
	add_child(lower)

	# Cabin set toward the rear so front and back are easy to tell apart.
	var cabin := MeshInstance3D.new()
	var cabin_mesh := BoxMesh.new()
	cabin_mesh.size = Vector3(width * 0.66, height * 0.42, length * 0.5)
	cabin.mesh = cabin_mesh
	cabin.material_override = _material(Color(0.12, 0.14, 0.18))
	cabin.position = Vector3(0.0, bottom + height * 0.71, -mid_forward + length * 0.1)
	add_child(cabin)
	cabin_node = cabin

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
		tire.material_override = _material(Color(0.07, 0.07, 0.08))
		# A cylinder's axis is local Y; the axle is local X.
		tire.rotation.z = PI * 0.5
		pivot.add_child(tire)
		# A light block on the wheel face so rotation is visible.
		var spoke := MeshInstance3D.new()
		var spoke_mesh := BoxMesh.new()
		spoke_mesh.size = Vector3(tire_width * 1.04, radius * 1.3, radius * 0.25)
		spoke.mesh = spoke_mesh
		spoke.material_override = _material(Color(0.75, 0.75, 0.78))
		pivot.add_child(spoke)
		wheels.append(pivot)


# The cabin box is hidden in the driver's-seat view so it does not block it.
func set_cabin_visible(on: bool) -> void:
	if cabin_node:
		cabin_node.visible = on


# `states` holds five numbers per wheel: local x, y, z, steer angle and
# spin angle (see SimBridge.wheel_states).
func update(car_xform: Transform3D, states: PackedFloat32Array) -> void:
	global_transform = car_xform
	for i in range(mini(wheels.size(), states.size() / 5)):
		var o := i * 5
		var pivot: Node3D = wheels[i]
		pivot.position = Vector3(states[o], states[o + 1], states[o + 2])
		# Default Euler order is YXZ: steer about Y, then roll about X.
		pivot.rotation = Vector3(-states[o + 4], states[o + 3], 0.0)
