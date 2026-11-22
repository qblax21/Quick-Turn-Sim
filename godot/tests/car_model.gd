# SPDX-License-Identifier: Apache-2.0
# Checks that the car view draws each car's Blender model (see
# tools/blender/make_car.py) with four model wheels, that a variant falls
# back to its base car's model, and that a car without one still gets the
# generated body.
#   godot --headless --path godot --script res://tests/car_model.gd
extends SceneTree

const CarView := preload("res://scripts/car_view.gd")

var failures: Array = []


func _check(ok: bool, what: String) -> void:
	print(("ok    " if ok else "FAIL  ") + what)
	if not ok:
		failures.append(what)


func _initialize() -> void:
	_run.call_deferred()


func _view(id: String) -> Node3D:
	var view = CarView.new()
	root.add_child(view)
	view.build({"id": id, "length": 3.95, "width": 1.73, "height": 1.28, "ground_clearance": 0.13,
		"cg_height": 0.56, "wheelbase": 2.5, "cg_to_front_axle": 0.93,
		"wheel_radius_front": 0.319, "wheel_radius_rear": 0.319, "tire_width_front": 0.225, "tire_width_rear": 0.225})
	return view


func _run() -> void:
	for id in ["hatch_tern_gt", "sedan_halden_e4"]:
		_check(CarView.model_path(id) == "res://assets/cars/%s.glb" % id, "%s has a model" % id)
		var view := _view(id)
		var model: Node = view.get_node_or_null("model")
		_check(model != null and model.find_child("body", true, false) != null, "%s draws the model's body" % id)
		_check(model != null and model.find_child("wheel_front", true, false) == null, "%s's wheel templates are not left on the body" % id)
		var wheels := 0
		var calipers := 0
		for pivot in view.wheels:
			var side: Node = pivot.get_node_or_null("side")
			if side and side.get_node_or_null("wheel") and side.get_node("wheel").mesh != null:
				wheels += 1
			if side and side.get_node_or_null("caliper") and side.get_node("caliper").mesh != null:
				calipers += 1
		_check(wheels == 4, "%s has four model wheels (got %d)" % [id, wheels])
		_check(calipers == 4, "%s has four brake calipers (got %d)" % [id, calipers])
		# Front wheels steered 0.3 rad and every wheel turned 1 rad.
		var states := PackedFloat32Array([-0.75, -0.24, -0.93, 0.3, 1, 0.75, -0.24, -0.93, 0.3, 1, -0.75, -0.24, 1.57, 0, 1, 0.75, -0.24, 1.57, 0, 1])
		view.update(Transform3D.IDENTITY, states)
		var fl: Node3D = view.wheels[0]
		_check(fl.get_node("side").scale.x < 0.0 and view.wheels[1].get_node("side").scale.x > 0.0, "%s mirrors the left wheels" % id)
		_check(is_equal_approx(fl.rotation.y, 0.3), "%s steers the front wheels" % id)
		_check(is_equal_approx(fl.get_node("side/wheel").rotation.x, -1.0), "%s spins the wheels" % id)
		_check(fl.get_node("side/caliper").rotation == Vector3.ZERO, "%s's calipers steer but do not spin" % id)
		view.free()
	_check(CarView.model_path("hatch_tern_gt_lsd") == "res://assets/cars/hatch_tern_gt.glb", "a variant uses its base car's model")
	_check(CarView.model_path("no_such_car") == "", "a car without a model has no path")
	var plain := _view("no_such_car")
	_check(plain.get_node_or_null("model") == null and plain.get_child_count() > 5, "a car without a model gets the generated body")
	plain.free()
	print("PASS" if failures.is_empty() else "FAILED: %d" % failures.size())
	quit(0 if failures.is_empty() else 1)
