# Procedural fixtures

These original MIT-licensed GLB files exercise import validation, rig
correspondence, retargeting, contact correction, skinning, and Library Preview
transitions. Simple geometry keeps expected transforms deterministic.

`retarget/` covers distinct rest frames and skin palettes. `motions/` includes
multiple clips, layered meshes, rigid/affine deformation, and varied timing.
Builders are `src/fixtures.rs`, `src/motion_fixtures.rs`, and `src/rig_corpus.rs`.
Tests also construct malformed inputs in memory.

The user-facing demo is `assets/demo/RobotExpressive.glb`.
