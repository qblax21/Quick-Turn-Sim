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
	_build_trackside(info, paths, sim.track_regions(), layer + 0.016)

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


# A flat strip of the given half-width along part of a path, optionally
# shifted sideways by `offset` (positive to the right of travel).
func _path_strip(path: Dictionary, from: float, to: float, half_width: float, height: float, offset: float = 0.0) -> MeshInstance3D:
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
		var base: Vector3 = points[i] + Vector3(0.0, height, 0.0) + side * offset
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
		# Clockwise seen from above, which is Godot's front face, so the strip
		# is lit from above.
		indices.append_array(PackedInt32Array([a, a + 2, a + 1, a + 1, a + 2, a + 3]))
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


# Points every `step` metres along a path.
func _sample_path(path: Dictionary, step: float) -> PackedVector3Array:
	var points: PackedVector3Array = path.points
	var distances: PackedFloat32Array = path.distances
	var out := PackedVector3Array()
	if points.size() < 2:
		return out
	var total: float = distances[distances.size() - 1]
	var i := 0
	var s := 0.0
	while s <= total:
		while i < distances.size() - 2 and distances[i + 1] < s:
			i += 1
		var span: float = distances[i + 1] - distances[i]
		var u := 0.0 if span <= 0.0 else clampf((s - distances[i]) / span, 0.0, 1.0)
		out.append(points[i].lerp(points[i + 1], u))
		s += step
	return out


# Trackside furniture that helps a driver judge distance and speed: painted
# edge lines, distance boards before the corners that end a straight, rubber
# on the fast line, and the trees and barriers the core placed. The trees
# and barriers are solid in the core; the rest is drawing only.
func _build_trackside(info: Dictionary, paths: Dictionary, regions: Array, height: float) -> void:
	var paint := _material(Color(0.93, 0.93, 0.90))
	for region in regions:
		if region.kind != "path" or region.surface != "tarmac" or not paths.has(region.path):
			continue
		if region.to - region.from < 150.0:
			continue
		for side in [-1.0, 1.0]:
			var line := _path_strip(paths[region.path], region.from, region.to, 0.08, height, side * (region.half_width - 0.3))
			if line:
				line.material_override = paint
				add_child(line)
		_build_boards(paths[region.path], region)
		_build_rubber(paths[region.path], region, height - 0.006)
	_build_trees()
	_build_walls()


func _wrap_index(k: int, n: int, closed: bool) -> int:
	return posmod(k, n) if closed else clampi(k, 0, n - 1)


# Distance boards 150, 100 and 50 m before each real corner that follows a
# straight, on the outside of the corner.
func _build_boards(path: Dictionary, region: Dictionary) -> void:
	var step := 5.0
	var pts := _sample_path(path, step)
	var n := pts.size()
	if n < 40:
		return
	var closed: bool = pts[0].distance_to(path.points[path.points.size() - 1]) < 3.0
	var turn := PackedFloat32Array()
	turn.resize(n)
	for j in range(n):
		var before := pts[j] - pts[_wrap_index(j - 1, n, closed)]
		var after := pts[_wrap_index(j + 1, n, closed)] - pts[j]
		var a := Vector2(before.x, before.z)
		var b := Vector2(after.x, after.z)
		turn[j] = a.angle_to(b) if a.length() > 0.5 and b.length() > 0.5 else 0.0
	var corner_turn := step / 250.0
	for j in range(n):
		if absf(turn[j]) <= corner_turn or absf(turn[_wrap_index(j - 1, n, closed)]) > corner_turn:
			continue
		# Length of the straight before this corner entry.
		var straight := 0
		while straight < 60 and absf(turn[_wrap_index(j - 1 - straight, n, closed)]) <= corner_turn:
			if not closed and j - 1 - straight <= 0:
				break
			straight += 1
		var straight_m := straight * step
		if straight_m < 100.0:
			continue
		# How far the corner turns in all.
		var total := 0.0
		var k := 0
		while k < 80 and absf(turn[_wrap_index(j + k, n, closed)]) > corner_turn:
			total += turn[_wrap_index(j + k, n, closed)]
			k += 1
		if absf(total) < 0.5:
			continue
		for distance in [150, 100, 50]:
			if distance > straight_m - 20.0:
				continue
			var at := _wrap_index(j - int(distance / step), n, closed)
			var ahead: Vector3 = pts[_wrap_index(at + 1, n, closed)] - pts[at]
			ahead.y = 0.0
			if ahead.length() < 0.5:
				continue
			ahead = ahead.normalized()
			var right := Vector3(-ahead.z, 0.0, ahead.x)
			# A positive total is a right-hand corner; the board goes on the left.
			var outside := right * -signf(total)
			_board(pts[at] + outside * (region.half_width + 1.8), ahead, str(distance))


func _board(at: Vector3, travel: Vector3, text: String) -> void:
	var ground: Vector3 = sim_ref.ground_heights(PackedVector3Array([at]))[0]
	var node := Node3D.new()
	add_child(node)
	node.position = Vector3(at.x, ground.y if has_terrain else at.y, at.z)
	# -Z points the way the cars travel, so +Z faces the driver.
	node.basis = Basis.looking_at(travel, Vector3.UP)
	var panel := MeshInstance3D.new()
	var panel_mesh := BoxMesh.new()
	panel_mesh.size = Vector3(1.5, 0.8, 0.05)
	panel.mesh = panel_mesh
	panel.material_override = _material(Color(0.96, 0.96, 0.94))
	panel.position.y = 1.35
	node.add_child(panel)
	for side in [-0.6, 0.6]:
		var post := MeshInstance3D.new()
		var post_mesh := BoxMesh.new()
		post_mesh.size = Vector3(0.07, 1.4, 0.07)
		post.mesh = post_mesh
		post.material_override = _material(Color(0.45, 0.45, 0.47))
		post.position = Vector3(side, 0.7, -0.03)
		node.add_child(post)
	var label := Label3D.new()
	label.text = text
	label.font_size = 220
	label.pixel_size = 0.003
	label.outline_size = 0
	label.modulate = Color(0.05, 0.05, 0.05)
	label.position = Vector3(0.0, 1.35, 0.035)
	node.add_child(label)


func _tree_mesh() -> ArrayMesh:
	var mesh := ArrayMesh.new()
	var trunk := CylinderMesh.new()
	trunk.top_radius = 0.16
	trunk.bottom_radius = 0.24
	trunk.height = 2.4
	trunk.radial_segments = 6
	trunk.rings = 1
	var crown := CylinderMesh.new()
	crown.top_radius = 0.0
	crown.bottom_radius = 1.7
	crown.height = 5.2
	crown.radial_segments = 7
	crown.rings = 1
	for part in [[trunk, 0.6, Color(0.30, 0.21, 0.13)], [crown, 4.2, Color(0.13, 0.30, 0.14)]]:
		var tool := SurfaceTool.new()
		tool.begin(Mesh.PRIMITIVE_TRIANGLES)
		tool.append_from(part[0], 0, Transform3D(Basis.IDENTITY, Vector3(0.0, part[1], 0.0)))
		tool.commit(mesh)
		mesh.surface_set_material(mesh.get_surface_count() - 1, _material(part[2]))
	return mesh


# The trees the core placed. They are solid there: each trunk is drawn at
# the radius the car collides with.
func _build_trees() -> void:
	if not sim_ref.has_method("track_trees"):
		return
	var spots: PackedVector3Array = sim_ref.track_trees()
	var sizes: PackedFloat32Array = sim_ref.track_tree_sizes()
	if spots.is_empty():
		return
	var rng := RandomNumberGenerator.new()
	rng.seed = 20
	var multi := MultiMesh.new()
	multi.transform_format = MultiMesh.TRANSFORM_3D
	multi.mesh = _tree_mesh()
	multi.instance_count = spots.size()
	for i in range(spots.size()):
		var size: float = sizes[i]
		var basis := Basis(Vector3.UP, rng.randf_range(0.0, TAU)).scaled(Vector3(size, size, size))
		multi.set_instance_transform(i, Transform3D(basis, spots[i] - Vector3(0.0, 0.3, 0.0)))
	var instance := MultiMeshInstance3D.new()
	instance.multimesh = multi
	add_child(instance)


# Barriers: a rail on posts along each solid piece.
func _build_walls() -> void:
	if not sim_ref.has_method("track_walls"):
		return
	var ends: PackedVector3Array = sim_ref.track_walls()
	var rail := _material(Color(0.78, 0.79, 0.80))
	var stripe := _material(Color(0.80, 0.16, 0.14))
	for i in range(0, ends.size() - 1, 2):
		var a := ends[i]
		var b := ends[i + 1]
		var length := a.distance_to(b)
		if length < 0.01:
			continue
		var piece := MeshInstance3D.new()
		var mesh := BoxMesh.new()
		# As thick as the solid barrier in the core.
		mesh.size = Vector3(0.5, 0.75, length)
		piece.mesh = mesh
		piece.material_override = stripe if (i / 2) % 2 == 0 else rail
		add_child(piece)
		piece.look_at_from_position((a + b) * 0.5 + Vector3(0.0, 0.375, 0.0), b + Vector3(0.0, 0.375, 0.0), Vector3.UP)


# One pass count of smoothing on the lateral offsets `d` of a line through
# the points `q + side * d`: each offset moves to where the line bends
# least, within the track's width. Repeated, this approaches the line of
# least curvature, which is close to the line a driver would take.
func _relax(q: PackedVector2Array, side: PackedVector2Array, d: PackedFloat32Array, limit: float, closed: bool, passes: int) -> void:
	var n := q.size()
	if n < 6:
		return
	for pass_index in range(passes):
		for i in range(n):
			if not closed and (i < 2 or i > n - 3):
				continue
			var a := posmod(i - 2, n)
			var b := posmod(i - 1, n)
			var c := posmod(i + 1, n)
			var e := posmod(i + 2, n)
			var near := q[b] + side[b] * d[b] + q[c] + side[c] * d[c]
			var far := q[a] + side[a] * d[a] + q[e] + side[e] * d[e]
			d[i] = clampf(((near * 4.0 - far) / 6.0 - q[i]).dot(side[i]), -limit, limit)


# Dark rubber laid along the line of least curvature through a paved
# region, heavier where the line turns hardest. It shows where the fast
# line runs; it is drawn only and does not change grip.
func _build_rubber(path: Dictionary, region: Dictionary, height: float) -> void:
	var step := 8.0
	var pts := _sample_path(path, step)
	var closed: bool = pts.size() > 2 and pts[0].distance_to(path.points[path.points.size() - 1]) < 3.0
	if closed and pts[pts.size() - 1].distance_to(pts[0]) < step * 0.5:
		pts.remove_at(pts.size() - 1)
	var n := pts.size()
	if n < 24:
		return
	var q := PackedVector2Array()
	var side := PackedVector2Array()
	for i in range(n):
		q.append(Vector2(pts[i].x, pts[i].z))
		var ahead: Vector3 = pts[_wrap_index(i + 1, n, closed)] - pts[_wrap_index(i - 1, n, closed)]
		side.append(Vector2(-ahead.z, ahead.x).normalized())
	var limit: float = maxf(region.half_width - 1.7, 0.0)
	# Coarse to fine, so long corners settle without thousands of passes.
	var d := PackedFloat32Array()
	for stride in [4, 2, 1]:
		var level_q := PackedVector2Array()
		var level_side := PackedVector2Array()
		for i in range(0, n, stride):
			level_q.append(q[i])
			level_side.append(side[i])
		var m := level_q.size()
		var level_d := PackedFloat32Array()
		level_d.resize(m)
		for j in range(m):
			if d.is_empty():
				level_d[j] = 0.0
			elif j % 2 == 0:
				level_d[j] = d[mini(j / 2, d.size() - 1)]
			else:
				var next := posmod(j / 2 + 1, d.size()) if closed else mini(j / 2 + 1, d.size() - 1)
				level_d[j] = (d[j / 2] + d[next]) * 0.5
		_relax(level_q, level_side, level_d, limit, closed, 400 / stride)
		d = level_d
	# The line, and how hard it turns at each point.
	var line := PackedVector2Array()
	for i in range(n):
		line.append(q[i] + side[i] * d[i])
	var vertices := PackedVector3Array()
	var colors := PackedColorArray()
	var rows := 0
	for i in range(n):
		var s := i * step
		if s < region.from or s > region.to:
			continue
		var before := line[i] - line[_wrap_index(i - 1, n, closed)]
		var after := line[_wrap_index(i + 1, n, closed)] - line[i]
		var bend := absf(before.angle_to(after)) / step if before.length() > 0.1 and after.length() > 0.1 else 0.0
		var alpha := 0.22 + 0.30 * clampf(bend * 45.0, 0.0, 1.0)
		var across := Vector2(-(before + after).y, (before + after).x).normalized()
		# Two tire tracks.
		for offset in [-0.98, -0.52, 0.52, 0.98]:
			var p: Vector2 = line[i] + across * offset
			vertices.append(Vector3(p.x, pts[i].y, p.y))
			colors.append(Color(0.03, 0.03, 0.03, alpha))
		rows += 1
	if rows < 2:
		return
	if has_terrain:
		vertices = sim_ref.ground_heights(vertices)
	for k in range(vertices.size()):
		var v := vertices[k]
		v.y += height
		vertices[k] = v
	var indices := PackedInt32Array()
	var joins := rows if closed and region.to - region.from > (n - 2) * step else rows - 1
	for r in range(joins):
		var a := r * 4
		var b := ((r + 1) % rows) * 4
		for t in [0, 2]:
			indices.append_array(PackedInt32Array([a + t, b + t, a + t + 1, a + t + 1, b + t, b + t + 1]))
	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = vertices
	arrays[Mesh.ARRAY_COLOR] = colors
	arrays[Mesh.ARRAY_INDEX] = indices
	var mesh := ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	var material := StandardMaterial3D.new()
	material.vertex_color_use_as_albedo = true
	material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	material.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	material.cull_mode = BaseMaterial3D.CULL_DISABLED
	var instance := MeshInstance3D.new()
	instance.mesh = mesh
	instance.material_override = material
	instance.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	add_child(instance)
