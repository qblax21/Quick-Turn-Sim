# SPDX-License-Identifier: Apache-2.0
# Development tool: renders the start menu and saves a screenshot.
#   godot --path godot --script res://tests/menu_shot.gd -- OUTPUT.png [setup]
extends SceneTree


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var args := OS.get_cmdline_user_args()
	var main = load("res://main.tscn").instantiate()
	root.add_child(main)
	for i in range(5):
		await process_frame
	if args.size() > 1 and args[1] == "setup":
		main._show_input_setup()
		for i in range(5):
			await process_frame
	await RenderingServer.frame_post_draw
	var error := root.get_viewport().get_texture().get_image().save_png(args[0])
	print("saved %s (%d)" % [args[0], error])
	quit(0 if error == OK else 1)
