# SPDX-License-Identifier: Apache-2.0
# Checks the title screen's drone shot is smooth (D-088): its height and
# aim against the earlier camera that rode the track's points directly.
#   godot --path godot --script res://tests/title_camera.gd
extends SceneTree


func _initialize() -> void:
	_run.call_deferred()


# The camera before D-088, for comparison.
func _old_pose(points: PackedVector3Array, s: float) -> Array:
	var n := points.size()
	var i := int(s)
	var at := points[i].lerp(points[(i + 1) % n], s - i)
	var ahead := points[(i + 40) % n]
	var along := (ahead - at)
	along.y = 0.0
	along = along.normalized() if along.length() > 0.01 else Vector3.FORWARD
	var side := Vector3(along.z, 0.0, -along.x)
	var pos := at + side * 25.0 + Vector3.UP * 22.0
	return [pos, (ahead + Vector3.UP * 2.0 - pos).normalized()]


# RMS of the second difference of height (bounce) and of the frame-to-frame
# turn of the view direction (jerk).
func _roughness(poses: Array) -> Array:
	var bounce := 0.0
	var turn := 0.0
	for k in range(2, poses.size()):
		bounce += pow(poses[k][0].y - 2.0 * poses[k - 1][0].y + poses[k - 2][0].y, 2)
		var a: float = poses[k][1].angle_to(poses[k - 1][1])
		var b: float = poses[k - 1][1].angle_to(poses[k - 2][1])
		turn += pow(a - b, 2)
	var n := float(poses.size() - 2)
	return [sqrt(bounce / n), sqrt(turn / n)]


func _run() -> void:
	var main = load("res://main.tscn").instantiate()
	root.add_child(main)
	for i in 5:
		await process_frame
	var points: PackedVector3Array = main.scenery_points
	var dt := 1.0 / 60.0
	var old_poses: Array = []
	var new_poses: Array = []
	var s := 0.0
	main.scenery_s = 0.0
	main.scenery_aim = Vector3.ZERO
	main.set_process(false)
	for k in 1200:
		s = fmod(s + dt * 9.0, float(points.size()))
		old_poses.append(_old_pose(points, s))
		main._fly_scenery(dt)
		new_poses.append([main.camera.global_position, -main.camera.global_transform.basis.z])
	var old_r := _roughness(old_poses.slice(60))
	var new_r := _roughness(new_poses.slice(60))
	print("bounce (m per frame^2): before %.5f, now %.5f" % [old_r[0], new_r[0]])
	print("aim jerk (rad per frame^2): before %.8f, now %.8f" % [old_r[1], new_r[1]])
	# Never rougher than before, and the aim far steadier.
	var ok: bool = new_r[0] <= old_r[0] + 1e-6 and new_r[1] < 0.2 * old_r[1]
	print("PASS" if ok else "FAIL")
	quit(0 if ok else 1)
