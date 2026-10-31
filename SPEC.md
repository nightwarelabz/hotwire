# Hotwire SPEC — the bridge contract

Version 0 (founding, 2026-10-30). This document is the contract
between three parties that never touch each other directly:

- **Blender** (the authoring tool, on the artist's machine)
- **Hotwire** (this repo: converter + validator + watcher)
- **The C35 host** (DeBrief: consumer, hot-reloads models live)

## The wire

```
Blender ──(built-in glTF export, .glb)──▶ drop folder
        ──(hotwire convert + era check)──▶ models folder (.c35m + manifest)
        ──(host watch: format::load_from, hot-swap)──▶ the world
```

- The payload format on the engine side is **`.c35m` v1**, the C35
  modeler's own text grammar (named materials, `v` lines, `f` lines
  with UVs). Hotwire writes it; the engine's existing
  `modeler::format::load_from` reads it. No new engine format.
- The interchange format is **glTF binary (.glb)** — chosen because
  Blender exports it natively. Hotwire v1 contains **zero bpy code**:
  the only Blender-side act is Blender's own exporter.
- A thin one-click Blender add-on ("Export & Push") may arrive later
  as a convenience. It lives in this repo, stays minimal, and is
  never a dependency of the pipeline.

## Conversion rules (v1)

- Coordinates: Blender Z-up → C35 Y-up (rotate −90° about X).
  Scale: 1 unit = 1 metre on both sides; no rescaling, ever.
- Geometry: triangulated; transforms baked; one `.c35m` per
  top-level named object (object name sanitized to the model's
  `name` line).
- Normals: flat per-face (the PS1.5 facet read), written explicitly.
- Materials: Blender material names pass through; an optional
  `hotwire.toml` mapping table renames them to house materials
  (`painted-metal`, `lamp-glass`, …). Unmapped names are reported,
  not silently kept.
- UVs: first UV layer. Textures: out of scope for v1 (C35 surfaces
  are palette/procedural grade); UVs still cross.
- Skinning/animation: out of scope for v1. A skinned input produces
  a clear refusal report, never a mangled model.
- Determinism: the same `.glb` in produces a byte-identical
  `.c35m` out (stable ordering, fixed float formatting matching
  the engine's own `format::save`).

## Era enforcement (the law, mechanically)

Per-asset budgets, configurable in `hotwire.toml`:

- Props: warn past 2,000 tris, refuse past 20,000.
- Characters: warn past 4,000 tris, refuse past 40,000.
- Every conversion reports: triangles, bounding box (metres),
  materials used, warnings. A refusal writes nothing to the
  models folder.

The golden standard is the banner: low triangles, high surface
craft. Hotwire's job is to make that discipline automatic at the
border, not to trust willpower.

## Manifest

Each conversion appends a line to `models/manifest.txt` in the
models folder: model name, source `.glb`, triangle count, and the
SHA-256 of the written `.c35m`. The manifest is the audit trail;
the host needs only the folder itself.

## Host contract

- The host watches the models folder (mtime poll is fine at v1).
- A changed `.c35m` is reloaded via `format::load_from` and the
  live mesh swapped in place; a failed load keeps the old mesh
  and reports — the world never goes down for a bad export.

## Phases

- **v1** — converter + era checks + manifest + host hot-reload +
  one proof asset end-to-end.
- **v2** — scene placement: named instances with transforms
  exported as a placement manifest, so whole blocks are dressed
  in Blender (the world-editor step).
- **Later, only if the feel demands it** — camera sync between
  the host and Blender's viewport.
