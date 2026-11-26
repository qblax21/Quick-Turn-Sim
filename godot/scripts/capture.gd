# SPDX-License-Identifier: Apache-2.0
# Renders the scene once through an off-screen viewport at a multiple of the
# window size and returns the image: high-resolution stills and trailer
# frames. It shares the scene's world, so the environment, lights and
# depth of field are the ones on screen, without any on-screen overlay.
extends RefCounted

# The largest viewport side the renderer is asked for.
const MAX_SIDE := 16384


static func render(owner: Node, source: Camera3D, scale: float) -> Image:
	var window := owner.get_viewport().get_visible_rect().size
	return await render_size(owner, source, Vector2i(roundi(window.x * scale), roundi(window.y * scale)))


# The same at an exact size in pixels, whatever the window's (D-129: the
# store's 1920x1080).
static func render_size(owner: Node, source: Camera3D, wanted: Vector2i) -> Image:
	var size := wanted.clamp(Vector2i(16, 16), Vector2i(MAX_SIDE, MAX_SIDE))
	var viewport := SubViewport.new()
	viewport.size = size
	viewport.own_world_3d = false
	viewport.world_3d = owner.get_viewport().world_3d
	viewport.render_target_update_mode = SubViewport.UPDATE_ONCE
	viewport.msaa_3d = owner.get_viewport().msaa_3d
	var camera := Camera3D.new()
	camera.fov = source.fov
	camera.near = source.near
	camera.far = source.far
	camera.attributes = source.attributes
	viewport.add_child(camera)
	owner.add_child(viewport)
	camera.global_transform = source.global_transform
	camera.current = true
	# One frame to draw it, one more so the texture is ready to read.
	await RenderingServer.frame_post_draw
	await RenderingServer.frame_post_draw
	var image := viewport.get_texture().get_image()
	viewport.queue_free()
	return image
