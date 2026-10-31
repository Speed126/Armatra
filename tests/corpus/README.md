# Retarget ground truth

`rig-manifest.json` records source-node → target-node ground truth before names
or node indices are changed. Regenerate with `armatra --write-retarget-samples`.
The generator is `src/rig_corpus.rs`; these original procedural assets use MIT.

Ten cases cover renamed and permuted nodes, different limb proportions, fully
rotated joint frames, helper/twist and extra spine subdivisions, missing toes
and fingers, unnamed joints, ambiguous names, nonuniform scales, and combined
changes. `review` lists intentionally ambiguous source rows. Correspondences
are ground truth, not the output of the assignment solver. Unsupported affine
retarget cases must be diagnosed rather than counted as successful transfers.
