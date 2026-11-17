# SPDX-License-Identifier: Apache-2.0
# Generates a low-poly car body from the dimensions the simulation core
# reports. The shape is original and generic: a set of cross-sections from
# nose to tail, joined into flat-shaded panels, with wheel wells cut in at
# the two axles. Drawing only.
extends RefCounted

# Cross-section keyframes from nose (0) to tail (1):
# [position, floor lift, shoulder height, roof height, width].
# Heights are fractions of the body height above the floor; width is a
# fraction of the overall width.
const THREE_BOX := [
	[0.00, 0.14, 0.46, 0.48, 0.82],
	[0.03, 0.03, 0.52, 0.54, 0.97],
	[0.08, 0.00, 0.56, 0.58, 1.00],
	[0.30, 0.00, 0.63, 0.65, 1.00],
	[0.45, 0.00, 0.66, 1.00, 1.00],
	[0.64, 0.00, 0.66, 0.99, 1.00],
	[0.86, 0.00, 0.68, 0.73, 1.00],
	[0.97, 0.04, 0.67, 0.70, 0.97],
	[1.00, 0.20, 0.60, 0.62, 0.84],
]
const TWO_BOX := [
	[0.00, 0.14, 0.46, 0.48, 0.82],
	[0.03, 0.03, 0.53, 0.55, 0.97],
	[0.08, 0.00, 0.57, 0.59, 1.00],
	[0.31, 0.00, 0.64, 0.66, 1.00],
	[0.47, 0.00, 0.67, 1.00, 1.00],
	[0.82, 0.00, 0.67, 0.97, 1.00],
	[0.96, 0.05, 0.66, 0.70, 0.98],
	[1.00, 0.22, 0.58, 0.60, 0.86],
]
# Cars shorter than this get the two-box (hatchback) shape.
const TWO_BOX_MAX_LENGTH := 4.3
# Gap between the tire and the wheel arch, m.
const ARCH_GAP := 0.07
const ARCH_STEPS := 8
const POINTS := 8

var keys: Array
var length: float
var width: float
var height: float
var floor_y: float
var nose_z: float
# Per axle: [z, wheel centre height, arch radius, tire width].
var axles: Array = []
var paint_vertices := PackedVector3Array()
var paint_normals := PackedVector3Array()
var glass_vertices := PackedVector3Array()
var glass_normals := PackedVector3Array()


func setup(info: Dictionary) -> void:
	length = info.get("length", 4.2)
	width = info.get("width", 1.8)
	height = info.get("height", 1.25)
	var clearance: float = info.get("ground_clearance", 0.13)
	var cg_height: float = info.get("cg_height", 0.5)
	var wheelbase: float = info.get("wheelbase", 2.6)
	var cg_to_front: float = info.get("cg_to_front_axle", 1.2)
	keys = TWO_BOX if length < TWO_BOX_MAX_LENGTH else THREE_BOX
	# The origin is the sprung centre of gravity; forward is -Z.
	floor_y = -(cg_height - clearance)
	nose_z = -(cg_to_front - wheelbase * 0.5) - length * 0.5
	for front in [true, false]:
		var radius: float = info.get("wheel_radius_front" if front else "wheel_radius_rear", 0.31)
		var tire_width: float = info.get("tire_width_front" if front else "tire_width_rear", 0.22)
		var z := -cg_to_front if front else wheelbase - cg_to_front
		axles.append([z, radius - cg_height, radius + ARCH_GAP, tire_width])


func _key_at(t: float) -> Array:
	for i in range(keys.size() - 1):
		var a: Array = keys[i]
		var b: Array = keys[i + 1]
		if t <= b[0]:
			var u := clampf((t - a[0]) / (b[0] - a[0]), 0.0, 1.0)
			return [t, lerpf(a[1], b[1], u), lerpf(a[2], b[2], u), lerpf(a[3], b[3], u), lerpf(a[4], b[4], u)]
	return keys[keys.size() - 1]


# The right half of the cross-section at `z`, from the bottom centre round
# to the roof centre. `axle` is the wheel well this section cuts through,
# or -1.
func section(z: float, axle: int = -1) -> PackedVector3Array:
	var key := _key_at((z - nose_z) / length)
	var half: float = width * 0.5 * key[4]
	var y_floor: float = floor_y + key[1] * height
	var y_shoulder: float = floor_y + key[2] * height
	var y_roof: float = floor_y + key[3] * height
	var low := y_floor
	var inner := half * 0.985
	if axle >= 0:
		var a: Array = axles[axle]
		var dz: float = z - a[0]
		var rise := sqrt(maxf(a[2] * a[2] - dz * dz, 0.0))
		low = maxf(low, minf(a[1] + rise, y_shoulder - 0.08))
		inner = minf(inner, width * 0.5 - a[3] - 0.09)
	var y_belt := maxf(lerpf(y_floor, y_shoulder, 0.45), low + 0.04)
	var tumble := clampf((y_roof - y_shoulder) / (0.25 * height), 0.0, 1.0)
	var points := PackedVector3Array()
	points.append(Vector3(0.0, y_floor, z))
	points.append(Vector3(inner, y_floor, z))
	points.append(Vector3(inner, low, z))
	points.append(Vector3(half * 0.985, low, z))
	points.append(Vector3(half, y_belt, z))
	points.append(Vector3(half * 0.94, y_shoulder, z))
	points.append(Vector3(half * lerpf(0.86, 0.74, tumble), y_roof, z))
	points.append(Vector3(0.0, y_roof + 0.02 * height, z))
	return points


func _axle_at(z: float) -> int:
	for i in range(axles.size()):
		if absf(z - axles[i][0]) < axles[i][2] - 0.0005:
			return i
	return -1


# Adds one triangle facing along `outward`.
func _triangle(a: Vector3, b: Vector3, c: Vector3, outward: Vector3, glass: bool) -> void:
	var normal := (b - a).cross(c - a)
	if normal.length_squared() < 1e-10:
		return
	if normal.dot(outward) < 0.0:
		var swap := b
		b = c
		c = swap
		normal = -normal
	normal = normal.normalized()
	var vertices := glass_vertices if glass else paint_vertices
	var normals := glass_normals if glass else paint_normals
	# Godot's front faces wind clockwise.
	vertices.append_array(PackedVector3Array([a, c, b]))
	normals.append_array(PackedVector3Array([normal, normal, normal]))
	if glass:
		glass_vertices = vertices
		glass_normals = normals
	else:
		paint_vertices = vertices
		paint_normals = normals


func _mirror(p: Vector3) -> Vector3:
	return Vector3(-p.x, p.y, p.z)


func build_mesh() -> ArrayMesh:
	# Stations: the keyframes, plus a ring of sections through each wheel
	# well. Each well begins and ends with a doubled station, which makes
	# its front and back walls.
	var stations: Array = []
	for key in keys:
		var z: float = nose_z + key[0] * length
		stations.append([z, 0, _axle_at(z)])
	for i in range(axles.size()):
		var a: Array = axles[i]
		stations.append([a[0] - a[2], 0, -1])
		stations.append([a[0] - a[2], 1, i])
		for k in range(1, ARCH_STEPS):
			stations.append([a[0] - a[2] * cos(PI * k / ARCH_STEPS), 0, i])
		stations.append([a[0] + a[2], 0, i])
		stations.append([a[0] + a[2], 1, -1])
	stations.sort_custom(func(p: Array, q: Array) -> bool:
		return p[0] < q[0] if absf(p[0] - q[0]) > 1e-6 else p[1] < q[1])

	var sections: Array = []
	for station in stations:
		sections.append(section(station[0], station[2]))

	for s in range(sections.size() - 1):
		var near: PackedVector3Array = sections[s]
		var far: PackedVector3Array = sections[s + 1]
		var dz: float = stations[s + 1][0] - stations[s][0]
		var wall := dz < 1e-6
		var well: int = maxi(stations[s][2], stations[s + 1][2])
		var greenhouse := (near[6].y - near[5].y + far[6].y - far[5].y) * 0.5 > 0.10 * height
		# A screen is a steep stretch of roof that reaches the greenhouse.
		var tall := maxf(near[6].y - near[5].y, far[6].y - far[5].y) > 0.2 * height
		var raked := not wall and tall and absf(far[6].y - near[6].y) / height > dz / length
		for k in range(POINTS - 1):
			var outward: Vector3
			if wall:
				# A well wall faces the wheel.
				outward = Vector3(0.0, 0.0, signf(axles[well][0] - stations[s][0]))
			else:
				var along := (near[k + 1] - near[k]) + (far[k + 1] - far[k])
				outward = Vector3(along.y, -along.x, 0.0)
			var glass := (k == 5 and greenhouse) or (k == 6 and raked)
			_triangle(near[k], near[k + 1], far[k + 1], outward, glass)
			_triangle(near[k], far[k + 1], far[k], outward, glass)
			var mirrored := Vector3(-outward.x, outward.y, outward.z)
			_triangle(_mirror(near[k]), _mirror(near[k + 1]), _mirror(far[k + 1]), mirrored, glass)
			_triangle(_mirror(near[k]), _mirror(far[k + 1]), _mirror(far[k]), mirrored, glass)

	# Nose and tail faces.
	for end in [[sections[0], -1.0], [sections[sections.size() - 1], 1.0]]:
		var ring: PackedVector3Array = end[0]
		var centre := Vector3(0.0, (ring[0].y + ring[7].y) * 0.5, ring[0].z)
		var outward := Vector3(0.0, 0.0, end[1])
		for k in range(POINTS - 1):
			_triangle(centre, ring[k], ring[k + 1], outward, false)
			_triangle(centre, _mirror(ring[k]), _mirror(ring[k + 1]), outward, false)

	var mesh := ArrayMesh.new()
	for surface in [[paint_vertices, paint_normals], [glass_vertices, glass_normals]]:
		var arrays := []
		arrays.resize(Mesh.ARRAY_MAX)
		arrays[Mesh.ARRAY_VERTEX] = surface[0]
		arrays[Mesh.ARRAY_NORMAL] = surface[1]
		mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	return mesh
