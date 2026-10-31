# Hotwire end-to-end proof asset: a Greyport dock bollard.
#
# Run headless:
#   blender --background --python make_bollard.py -- <out.glb>
#
# Builds ONE object ("dock-bollard") from primitives in Blender's
# native Z-up, one material named "painted-metal", and exports
# .glb with +Y Up DISABLED — the exported file is genuinely Z-up,
# which is exactly what Hotwire's SPEC conversion (Z-up -> Y-up,
# rotate -90 about X) expects to receive.

import sys

import bpy


def main() -> None:
    argv = sys.argv[sys.argv.index("--") + 1:]
    out_path = argv[0]

    bpy.ops.wm.read_factory_settings(use_empty=True)

    # Body: a stout cylinder, 0.30 m across, 0.72 m tall.
    bpy.ops.mesh.primitive_cylinder_add(
        radius=0.15, depth=0.72, location=(0.0, 0.0, 0.36)
    )
    body = bpy.context.active_object
    body.name = "dock-bollard"

    # Cap: a squashed UV sphere as the dome, joined into the body
    # so the export is a single named object.
    bpy.ops.mesh.primitive_uv_sphere_add(radius=0.155, location=(0.0, 0.0, 0.72))
    cap = bpy.context.active_object
    cap.scale = (1.0, 1.0, 0.55)
    bpy.context.view_layer.objects.active = cap
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)

    bpy.ops.object.select_all(action="DESELECT")
    body.select_set(True)
    cap.select_set(True)
    bpy.context.view_layer.objects.active = body
    bpy.ops.object.join()

    bollard = bpy.context.active_object
    bollard.name = "dock-bollard"

    # One house material, by name (SPEC: material names carry).
    mat = bpy.data.materials.new(name="painted-metal")
    bollard.data.materials.append(mat)
    for poly in bollard.data.polygons:
        poly.material_index = 0

    bpy.ops.export_scene.gltf(
        filepath=out_path,
        export_format="GLB",
        export_yup=False,  # keep the file Z-up; Hotwire does the axis law
    )
    print(f"hotwire-proof: wrote {out_path}")


main()
