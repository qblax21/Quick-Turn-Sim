# SPDX-License-Identifier: Apache-2.0
# Builds placeholder geometry for a track from what the simulation core
# reports: ground, surface regions, cones, gates and painted markers. Colours
# and one generated grain texture only; no art.
extends Node3D

const SURFACE_COLORS := {
	"tarmac": Color(0.23, 0.24, 0.26),
	"gravel": Color(0.62, 0.55, 0.42),
	"dirt": Color(0.36, 0.42, 0.24),
	"kerb": Color(0.80, 0.22, 0.20),
}
const CONE_COLOR := Color(1.0, 0.45, 0.05)
const CONE_DOWN_COLOR := Color(0.45, 0.2, 0.05)

var cone_nodes: Array = []
var cone_was_down := PackedByteArray()
var sim_ref
var has_terrain := false
# The terrain mesh is drawn this far below the true ground so its coarse
# grid never pokes through the exactly shaped road.
const TERRAIN_DROP := 0.08
var grain: NoiseTexture2D


func _material(color: Color) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.albedo_color = color
	m.cull_mode = BaseMaterial3D.CULL_DISABLED
	m.roughness = 0.95
	return m


# Ground material: the surface colour times a generated grain, mapped in world
# space so the ground visibly moves under the car.
func _ground_material(color: Color) -> StandardMaterial3D:
	if grain == null:
		var noise := FastNoiseLite.new()
		noise.seed = 7
		noise.frequency = 0.06
		noise.fractal_octaves = 4
		var ramp := Gradient.new()
		ramp.set_color(0, Color(0.62, 0.62, 0.62))
		ramp.set_color(1, Color(1.0, 1.0, 1.0))
		grain = NoiseTexture2D.new()
		grain.width = 512
		grain.height = 512
		grain.seamless = true
		grain.noise = noise
		grain.color_ramp = ramp
	var m := _material(color)
	m.albedo_texture = grain
	m.uv1_triplanar = true
	m.uv1_world_triplanar = true
	m.uv1_scale = Vector3(0.11, 0.11, 0.11)
	return m


func _surface_color(id: String) -> Color:
	return SURFACE_COLORS.get(id, Color(0.5, 0.5, 0.5))


func build(sim) -> void:
	var info: Dictionary = sim.track_info()
	var extent: float = info.get("ground_half_extent", 200.0)

	var ground := MeshInstance3D.new()
	var plane := PlaneMesh.new()
	plane.size = Vector2(extent * 2.0, extent * 2.0)
	ground.mesh = plane
	ground.material_override = _ground_material(_surface_color(info.get("default_surface", "tarmac")))
	add_child(ground)
	sim_ref = sim
	has_terrain = info.get("has_terrain", false)
	ground.visible = not has_terrain
	# Grass out to the horizon around the drawn ground.
	var surround := MeshInstance3D.new()
	var surround_plane := PlaneMesh.new()
	surround_plane.size = Vector2(6000.0, 6000.0)
	surround.mesh = surround_plane
	surround.material_override = _ground_material(Color(0.30, 0.42, 0.22))
	surround.position.y = -0.03
	add_child(surround)

	var paths := {}
	for path in sim.track_paths():
		paths[path.id] = path
	if has_terrain:
		surround.position.y = _build_terrain(paths, _surface_color(info.get("default_surface", "tarmac")))

	# Regions are stacked slightly above the ground in the order the core
	# applies them, so later ones draw on top.
	var layer := 0.004
	for region in sim.track_regions():
		var color := _surface_color(region.surface)
		match region.kind:
			"rect":
				var box := MeshInstance3D.new()
				var mesh := BoxMesh.new()
				var height: float = maxf(region.ridge_height, 0.004)
				# The long axis of the region lies along local -Z (forward).
				mesh.size = Vector3(region.half_width * 2.0, height, region.half_length * 2.0)
				box.mesh = mesh
				box.material_override = _ground_material(color)
				box.position = region.center + Vector3(0.0, layer + height * 0.5, 0.0)
				box.rotation.y = region.heading
				add_child(box)
			"circle":
				var disc := MeshInstance3D.new()
				var cyl := CylinderMesh.new()
				cyl.top_radius = region.radius
				cyl.bottom_radius = region.radius
				cyl.height = 0.004
				disc.mesh = cyl
				disc.material_override = _ground_material(color)
				disc.position = region.center + Vector3(0.0, layer, 0.0)
				add_child(disc)
			"path":
				if paths.has(region.path):
					var strip := _path_strip(paths[region.path], region.from, region.to, region.half_width, layer)
					if strip:
						strip.material_override = _ground_material(color)
						add_child(strip)
		layer += 0.004

	# Cones.
	var cone_mesh := CylinderMesh.new()
	cone_mesh.top_radius = 0.03
	cone_mesh.bottom_radius = 0.14
	cone_mesh.height = 0.45
	var band_mesh := CylinderMesh.new()
	band_mesh.top_radius = 0.072
	band_mesh.bottom_radius = 0.094
	band_mesh.height = 0.09
	var base_mesh := BoxMesh.new()
	base_mesh.size = Vector3(0.3, 0.02, 0.3)
	var white := _material(Color(0.95, 0.95, 0.95))
	for p in sim.track_cones():
		var cone := MeshInstance3D.new()
		cone.mesh = cone_mesh
		cone.material_override = _material(CONE_COLOR)
		cone.position = p + Vector3(0.0, 0.225, 0.0)
		# White band and a square base, as children so they tip over with it.
		var band := MeshInstance3D.new()
		band.mesh = band_mesh
		band.material_override = white
		band.position.y = 0.04
		cone.add_child(band)
		var base := MeshInstance3D.new()
		base.mesh = base_mesh
		base.material_override = _material(CONE_COLOR)
		base.position.y = -0.215
		cone.add_child(base)
		add_child(cone)
		cone_nodes.append(cone)
	cone_was_down.resize(cone_nodes.size())

	# Timing gates: two posts and a strip on the ground.
	for gate in sim.track_gates():
		var color := Color(0.2, 0.9, 0.3) if gate.kind == "start" else Color(0.95, 0.95, 0.95)
		for end in [gate.a, gate.b]:
			var post := MeshInstance3D.new()
			var post_mesh := CylinderMesh.new()
			post_mesh.top_radius = 0.06
			post_mesh.bottom_radius = 0.06
			post_mesh.height = 1.6
			post.mesh = post_mesh
			post.material_override = _material(color)
			post.position = end + Vector3(0.0, 0.8, 0.0)
			add_child(post)
		add_child(_ground_line(gate.a, gate.b, 0.25, color, layer + 0.01))

	# Painted markers.
	for marker in sim.track_markers():
		var paint := Color(0.92, 0.92, 0.88)
		if marker.kind == "line":
			add_child(_ground_line(marker.a, marker.b, 0.15, paint, layer + 0.008))
		elif marker.kind == "circle":
			var segments := 96
			for i in range(segments):
				var a0 := TAU * i / segments
				var a1 := TAU * (i + 1) / segments
				var p0: Vector3 = marker.center + Vector3(cos(a0), 0.0, sin(a0)) * marker.radius
				var p1: Vector3 = marker.center + Vector3(cos(a1), 0.0, sin(a1)) * marker.radius
				add_child(_ground_line(p0, p1, 0.15, paint, layer + 0.008))


# Terrain as a grid of heights asked from the core, covering the paths and a
# margin around them. Toward its edge it levels out to a plain just below
# its lowest point; the height of that plain is returned so the surrounding
# plane can meet it. Drawing only.
func _build_terrain(paths: Dictionary, color: Color) -> float:
	var lo := Vector2(INF, INF)
	var hi := Vector2(-INF, -INF)
	for id in paths:
		var path_points: PackedVector3Array = paths[id].points
		for p in path_points:
			lo.x = minf(lo.x, p.x)
			lo.y = minf(lo.y, p.z)
			hi.x = maxf(hi.x, p.x)
			hi.y = maxf(hi.y, p.z)
	var margin := 350.0
	var step := 5.0
	lo -= Vector2(margin, margin)
	hi += Vector2(margin, margin)
	var nx := int(ceil((hi.x - lo.x) / step)) + 1
	var nz := int(ceil((hi.y - lo.y) / step)) + 1
	var points := PackedVector3Array()
	points.resize(nx * nz)
	for j in range(nz):
		for i in range(nx):
			points[j * nx + i] = Vector3(lo.x + i * step, 0.0, lo.y + j * step)
	points = sim_ref.ground_heights(points)
	# Level the edge of the grid out to a plain just below the lowest point.
	var far := INF
	for p in points:
		far = minf(far, p.y)
	far -= 1.0
	for k in range(points.size()):
		var p := points[k]
		var edge := minf(minf(p.x - lo.x, hi.x - p.x), minf(p.z - lo.y, hi.y - p.z))
		var t := clampf(edge / (margin * 0.6), 0.0, 1.0)
		t = t * t * (3.0 - 2.0 * t)
		p.y = lerpf(far, p.y, t) - TERRAIN_DROP
		points[k] = p
	var normals := PackedVector3Array()
	normals.resize(nx * nz)
	var indices := PackedInt32Array()
	for j in range(nz):
		for i in range(nx):
			var k := j * nx + i
			var west := points[k - 1].y if i > 0 else points[k].y
			var east := points[k + 1].y if i < nx - 1 else points[k].y
			var north := points[k - nx].y if j > 0 else points[k].y
			var south := points[k + nx].y if j < nz - 1 else points[k].y
			normals[k] = Vector3(west - east, 2.0 * step, north - south).normalized()
			if i < nx - 1 and j < nz - 1:
				indices.append_array(PackedInt32Array([k, k + 1, k + nx, k + 1, k + nx + 1, k + nx]))
	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = points
	arrays[Mesh.ARRAY_NORMAL] = normals
	arrays[Mesh.ARRAY_INDEX] = indices
	var mesh := ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	var instance := MeshInstance3D.new()
	instance.mesh = mesh
	instance.material_override = _ground_material(color)
	add_child(instance)
	return far - TERRAIN_DROP - 0.05


# A flat strip of the given half-width along part of a path.
func _path_strip(path: Dictionary, from: float, to: float, half_width: float, height: float) -> MeshInstance3D:
	var points: PackedVector3Array = path.points
	var distances: PackedFloat32Array = path.distances
	var vertices := PackedVector3Array()
	var normals := PackedVector3Array()
	var indices := PackedInt32Array()
	var count := points.size()
	for i in range(count):
		if distances[i] < from - 1.0 or distances[i] > to + 1.0:
			continue
		var ahead: Vector3 = points[mini(i + 1, count - 1)] - points[maxi(i - 1, 0)]
		ahead.y = 0.0
		if ahead.length() < 0.0001:
			continue
		ahead = ahead.normalized()
		var side := Vector3(-ahead.z, 0.0, ahead.x)
		var base: Vector3 = points[i] + Vector3(0.0, height, 0.0)
		vertices.append(base - side * half_width)
		vertices.append(base + side * half_width)
		normals.append(Vector3.UP)
		normals.append(Vector3.UP)
	if has_terrain:
		# Put both edges on the banked road surface.
		vertices = sim_ref.ground_heights(vertices)
		for k in range(vertices.size()):
			var v := vertices[k]
			v.y += height
			vertices[k] = v
	var rows := vertices.size() / 2
	if rows < 2:
		return null
	for r in range(rows - 1):
		var a := r * 2
		indices.append_array(PackedInt32Array([a, a + 1, a + 2, a + 1, a + 3, a + 2]))
	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = vertices
	arrays[Mesh.ARRAY_NORMAL] = normals
	arrays[Mesh.ARRAY_INDEX] = indices
	var mesh := ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	var instance := MeshInstance3D.new()
	instance.mesh = mesh
	return instance


# A thin painted line on the ground between two points.
func _ground_line(a: Vector3, b: Vector3, width: float, color: Color, height: float) -> MeshInstance3D:
	var line := MeshInstance3D.new()
	var mesh := BoxMesh.new()
	var length := a.distance_to(b)
	mesh.size = Vector3(width, 0.004, maxf(length, 0.01))
	line.mesh = mesh
	line.material_override = _material(color)
	line.position = (a + b) * 0.5 + Vector3(0.0, height, 0.0)
	var direction := b - a
	line.rotation.y = atan2(-direction.x, -direction.z)
	return line


# Tips over cones the core reports as knocked down.
func update_cones(down: PackedByteArray) -> void:
	for i in range(mini(down.size(), cone_nodes.size())):
		if down[i] == cone_was_down[i]:
			continue
		cone_was_down[i] = down[i]
		var cone: MeshInstance3D = cone_nodes[i]
		if down[i] == 1:
			cone.rotation.x = PI * 0.5
			cone.position.y = 0.14
			cone.material_override = _material(CONE_DOWN_COLOR)
		else:
			cone.rotation.x = 0.0
			cone.position.y = 0.225
			cone.material_override = _material(CONE_COLOR)
