# SPDX-License-Identifier: Apache-2.0
# SCons profile for the slimmed Godot 4.7.2 Windows release template (D-121).
# Used by tools/build_template.ps1 as `scons profile=<this file>`.
#
# Kept: Vulkan (Forward+), GDScript, GDExtension, 3D, GPU particles, the
# default theme (its icons are SVG, so the svg module stays), FreeType,
# glslang (Vulkan compiles shaders at run time), noise (FastNoiseLite makes the ground and sky textures), PNG and JPG
# saving (photo mode, bench), WebP (lossless texture storage), SDL3 (wheel
# and pedal input), BC decoding.
# Left out: everything below. The simulation is in our Rust library, so
# Godot's own physics is not used.

target = "template_release"
platform = "windows"
arch = "x86_64"
production = "yes"
debug_symbols = "no"

# Renderers: Forward+ on Vulkan only. No OpenGL fallback (the minimum spec
# already needs Vulkan) and no Direct3D 12.
vulkan = "yes"
opengl3 = "no"
d3d12 = "no"  # also on the command line: the platform turns it on otherwise

# Engine features with no use here.
disable_physics_2d = "yes"
disable_physics_3d = "yes"
disable_navigation_2d = "yes"
disable_navigation_3d = "yes"
disable_xr = "yes"
accesskit = "no"
brotli = "no"
minizip = "no"

# The full text server stays: with the fallback one (no ICU or HarfBuzz)
# wrapped labels overlapped in the first build (the title screen's footer).
module_text_server_adv_enabled = "yes"
module_text_server_fb_enabled = "no"

# Modules with no use here. Plain assignments: SCons reads this file with
# exec into a dictionary of its own.
module_astcenc_enabled = "no"
module_basis_universal_enabled = "no"
module_betsy_enabled = "no"
module_bmp_enabled = "no"
module_camera_enabled = "no"
module_csg_enabled = "no"
module_cvtt_enabled = "no"
module_dds_enabled = "no"
module_enet_enabled = "no"
module_etcpak_enabled = "no"
module_fbx_enabled = "no"
module_gltf_enabled = "no"
module_godot_physics_2d_enabled = "no"
module_godot_physics_3d_enabled = "no"
module_gridmap_enabled = "no"
module_hdr_enabled = "no"
module_interactive_music_enabled = "no"
module_jolt_physics_enabled = "no"
module_jsonrpc_enabled = "no"
module_ktx_enabled = "no"
module_lightmapper_rd_enabled = "no"
module_mbedtls_enabled = "no"
module_meshoptimizer_enabled = "no"
module_mobile_vr_enabled = "no"
module_mp3_enabled = "no"
module_msdfgen_enabled = "no"
module_multiplayer_enabled = "no"
module_navigation_2d_enabled = "no"
module_navigation_3d_enabled = "no"
module_objectdb_profiler_enabled = "no"
module_ogg_enabled = "no"
module_openxr_enabled = "no"
module_raycast_enabled = "no"
module_regex_enabled = "no"
module_tga_enabled = "no"
module_theora_enabled = "no"
module_tinyexr_enabled = "no"
module_upnp_enabled = "no"
module_vhacd_enabled = "no"
module_visual_shader_enabled = "no"
module_vorbis_enabled = "no"
module_webrtc_enabled = "no"
module_websocket_enabled = "no"
module_webxr_enabled = "no"
module_xatlas_unwrap_enabled = "no"
module_zip_enabled = "no"
