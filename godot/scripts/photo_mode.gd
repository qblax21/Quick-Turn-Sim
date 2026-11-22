# SPDX-License-Identifier: Apache-2.0
# Photo mode: a free camera over a frozen scene, with field of view, depth
# of field and high-resolution capture. It only moves a camera; whoever
# opens it pauses the simulation or the replay first, so nothing it does
# reaches the physics.
#
#   W A S D      move          Q / E   down / up      Shift  faster
#   right mouse  look          wheel   field of view
#   Tab          hide or show this panel
#   Enter / F12  capture       Esc / F8 / O   leave
extends Node

const Capture := preload("res://scripts/capture.gd")

signal closed

var camera: Camera3D
var scene_camera: Camera3D
var attributes: CameraAttributesPractical
var looking := false
var yaw := 0.0
var pitch := 0.0
var scale_index := 1
const SCALES := [1.0, 2.0, 4.0]
var last_saved := ""
var busy := false

var layer: CanvasLayer
var panel: PanelContainer
var status: Label
var sliders := {}


# Takes over from `source`, starting where it is looking.
func setup(source: Camera3D) -> void:
	scene_camera = source


func _ready() -> void:
	camera = Camera3D.new()
	camera.fov = scene_camera.fov
	camera.near = scene_camera.near
	camera.far = scene_camera.far
	attributes = CameraAttributesPractical.new()
	attributes.dof_blur_far_enabled = false
	attributes.dof_blur_near_enabled = false
	camera.attributes = attributes
	add_child(camera)
	camera.global_transform = scene_camera.global_transform
	camera.current = true
	var forward := -camera.global_transform.basis.z
	yaw = atan2(-forward.x, -forward.z)
	pitch = asin(clampf(forward.y, -1.0, 1.0))
	_build_panel()


func _build_panel() -> void:
	layer = CanvasLayer.new()
	layer.layer = 30
	add_child(layer)
	panel = PanelContainer.new()
	panel.position = Vector2(20, 20)
	layer.add_child(panel)
	var box := VBoxContainer.new()
	box.custom_minimum_size = Vector2(360, 0)
	panel.add_child(box)
	var title := Label.new()
	title.text = "PHOTO MODE"
	title.add_theme_font_size_override("font_size", 20)
	box.add_child(title)
	_slider(box, "fov", "Field of view (deg)", 5.0, 110.0, 1.0, camera.fov)
	var dof := CheckBox.new()
	dof.text = "Depth of field"
	dof.toggled.connect(func(on: bool) -> void:
		attributes.dof_blur_far_enabled = on
		attributes.dof_blur_near_enabled = on)
	box.add_child(dof)
	_slider(box, "focus", "Focus distance (m)", 0.5, 200.0, 0.5, 10.0)
	_slider(box, "blur", "Blur amount", 0.0, 1.0, 0.01, 0.15)
	var res := OptionButton.new()
	for s in SCALES:
		res.add_item("Capture at %dx the window" % int(s))
	res.select(scale_index)
	res.item_selected.connect(func(i: int) -> void: scale_index = i)
	box.add_child(res)
	var shoot := Button.new()
	shoot.text = "Capture (Enter)"
	shoot.pressed.connect(capture)
	box.add_child(shoot)
	status = Label.new()
	status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	status.add_theme_font_size_override("font_size", 13)
	status.text = "WASD move, Q/E down/up, Shift faster, right mouse look, wheel zoom. Tab hides this panel. Esc leaves."
	box.add_child(status)
	_apply()


func _slider(parent: Control, key: String, label_text: String, lo: float, hi: float, step: float, value: float) -> void:
	var label := Label.new()
	label.add_theme_font_size_override("font_size", 13)
	parent.add_child(label)
	var s := HSlider.new()
	s.min_value = lo
	s.max_value = hi
	s.step = step
	s.value = value
	s.value_changed.connect(func(v: float) -> void:
		label.text = "%s: %s" % [label_text, str(snappedf(v, step))]
		_apply())
	label.text = "%s: %s" % [label_text, str(snappedf(value, step))]
	parent.add_child(s)
	sliders[key] = s


func _apply() -> void:
	if sliders.is_empty():
		return
	camera.fov = sliders.fov.value
	var focus: float = sliders.focus.value
	# A band around the focus distance stays sharp; it widens with distance
	# as a real lens's depth of field does.
	var band := maxf(0.15 * focus, 0.3)
	attributes.dof_blur_far_distance = focus + band
	attributes.dof_blur_far_transition = maxf(focus, 1.0)
	attributes.dof_blur_near_distance = maxf(focus - band, 0.05)
	attributes.dof_blur_near_transition = maxf(0.5 * focus, 0.1)
	attributes.dof_blur_amount = sliders.blur.value


# Camera settings to apply from outside (tests and the director).
func set_view(fov: float, focus: float, blur: float, dof: bool) -> void:
	sliders.fov.value = fov
	sliders.focus.value = focus
	sliders.blur.value = blur
	attributes.dof_blur_far_enabled = dof
	attributes.dof_blur_near_enabled = dof
	_apply()


func _process(delta: float) -> void:
	var move := Vector3.ZERO
	if Input.is_physical_key_pressed(KEY_W):
		move.z -= 1.0
	if Input.is_physical_key_pressed(KEY_S):
		move.z += 1.0
	if Input.is_physical_key_pressed(KEY_A):
		move.x -= 1.0
	if Input.is_physical_key_pressed(KEY_D):
		move.x += 1.0
	if Input.is_physical_key_pressed(KEY_E):
		move.y += 1.0
	if Input.is_physical_key_pressed(KEY_Q):
		move.y -= 1.0
	var speed := 25.0 if Input.is_physical_key_pressed(KEY_SHIFT) else 5.0
	camera.basis = Basis.from_euler(Vector3(pitch, yaw, 0.0))
	if move != Vector3.ZERO:
		camera.global_position += camera.global_transform.basis * move.normalized() * speed * delta


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		if event.button_index == MOUSE_BUTTON_RIGHT:
			looking = event.pressed
		elif event.pressed and event.button_index == MOUSE_BUTTON_WHEEL_UP:
			sliders.fov.value -= 2.0
		elif event.pressed and event.button_index == MOUSE_BUTTON_WHEEL_DOWN:
			sliders.fov.value += 2.0
	elif event is InputEventMouseMotion and looking:
		yaw -= event.relative.x * 0.004
		pitch = clampf(pitch - event.relative.y * 0.004, -1.5, 1.5)
	elif event is InputEventKey and event.pressed and not event.echo:
		match event.physical_keycode:
			KEY_TAB:
				panel.visible = not panel.visible
			KEY_ENTER, KEY_KP_ENTER, KEY_F12:
				capture()
			KEY_ESCAPE, KEY_F8, KEY_O:
				leave()
			_:
				return
	else:
		return
	get_viewport().set_input_as_handled()


# Saves a still at the chosen multiple of the window size.
func capture() -> void:
	if busy:
		return
	busy = true
	var image: Image = await Capture.render(self, camera, SCALES[scale_index])
	var dir := ProjectSettings.globalize_path("user://photos")
	DirAccess.make_dir_recursive_absolute(dir)
	var stamp := Time.get_datetime_string_from_system().replace(":", "-")
	last_saved = dir.path_join("photo_%s_%dx%d.png" % [stamp, image.get_width(), image.get_height()])
	var error := image.save_png(last_saved)
	status.text = ("saved %s" % last_saved) if error == OK else "could not save the photo (error %d)" % error
	busy = false


func leave() -> void:
	scene_camera.current = true
	closed.emit()
	queue_free()
