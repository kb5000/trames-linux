# Desktop shaders

These project-owned GPL-3.0-or-later shaders compose retained native GPU planes
and uploaded SHM planes. The checked-in SPIR-V allows ordinary Rust builds to
remain independent of a shader compiler. Rebuild with:

```sh
glslangValidator -V --target-env vulkan1.0 desktop.vert -o desktop.vert.spv
glslangValidator -V --target-env vulkan1.0 desktop.frag -o desktop.frag.spv
```

The 48-byte push record carries destination bounds, normalized crop and
transform/alpha/color-encoding flags. Textures use premultiplied alpha. Production
GPU planes remain on their original device/queue through the submission fence.
