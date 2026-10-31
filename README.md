# Hotwire

**The C35 × Blender bridge.** Model in Blender, walk it in Greyport.

Hotwire is NIGHTWARE LABZ's own bridge between Blender and the C35
engine. It is deliberately *not* part of either: no Blender code, no
engine code — only the wire between them, built with our own tech so
nothing about the link is borrowed, licensed-in, or owed to anyone.

## How it works

1. **Blender** authors the model. Export uses Blender's own built-in
   glTF exporter — Hotwire contains zero Blender-API code.
2. **Hotwire** watches a drop folder, converts each `.glb` into the
   engine's native `.c35m` model format, and enforces the house era
   law mechanically: triangle budgets, flat facets, named materials,
   honest scale. What doesn't pass doesn't cross.
3. **The C35 host** (DeBrief) watches the models folder and hot-swaps
   the mesh in the running game. Edit, export, walk around it.

The full contract is in [SPEC.md](SPEC.md).

## Why a separate repo

The bridge is a tool with its own life: it serves the engine without
living in it, and it faces Blender without depending on it. C35 stays
100% from-scratch; Blender stays on the artist's machine; Hotwire is
the only thing that touches both worlds, and all it carries is data.

## Status

Founding — spec first (2026-10-30). Converter v1 in build:
glTF → `.c35m` with era enforcement, folder watch, host hot-reload.
