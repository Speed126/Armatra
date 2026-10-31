# Demo assets and fonts

`demo/RobotExpressive.glb` is RobotExpressive by Tomás Laulhé (Quaternius), with
glTF modifications by Don McCurdy, under CC0 1.0. Creator credits remain in
[the upstream notice](demo/RobotExpressive-SOURCE.md), alongside the
[complete CC0 text](demo/CC0-1.0.txt).

Pinned upstream:
[model and notice](https://github.com/mrdoob/three.js/tree/cb0a902b668f848b2a0b6e1fb8f928f5a8ef1168/examples/models/gltf/RobotExpressive).
The adaptation removes unsupported facial morph channels, defaults, and targets.
Skeletal TRS animation, hierarchy, skin palettes, geometry, and materials are
retained. It has no image textures. From the source root,
`python scripts/prepare-demo.py original.glb output.glb` reproduces the adaptation
from the checked upstream SHA-256.

`demo/motions/*.glb` are original MIT-licensed procedural skeletons and curves,
without character meshes. Their builders are `src/fixtures.rs`,
`src/motion_fixtures.rs`, and `src/rig_corpus.rs`. Procedural regression fixtures
in `tests/fixtures` use the same project license.

`fonts/ArmatraUI-400.ttf`, `-500.ttf`, and `-600.ttf` are static Inter instances,
with the family renamed Armatra UI. They retain the Inter Project Authors'
copyright and [SIL OFL 1.1](fonts/Inter-LICENSE.txt).
See [font details](fonts/README.md) and [dependency notices](../docs/third-party/README.md).

The README GIFs use direct captures of Armatra's composed native winit/wgpu
window with this CC0 robot, original motions, interface, and fonts. Playback
runs at normal speed; captures are downsampled to 1080 pixels wide and 15 fps.
The Library and retarget clips join short native recordings. Interface and
model content are not composited.

The retarget example uses the procedural Wave motion with a renamed clip label
and a saved manual mapping to the robot's selected skin. It is a different
source skeleton, not the robot's native Wave clip. Unmapped joints stay at rest.
The application/documentation portions use MIT; component licenses apply to
their respective content.
