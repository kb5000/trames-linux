# Mac-hosted Linux GPU test VM

Status, 2026-11-25: candidate assessment; no new VM, application or image has
been downloaded. The existing Lima 2.2.1 VZ VM remains the ARM64/4 KiB/nested-KVM
reference. This Mac runs macOS 27.0.1; UTM, QEMU and krunkit are not installed.

## Candidates

| Candidate | Published capability | What still needs proof here |
| --- | --- | --- |
| UTM 5 QEMU + VirtIO Venus | Linux Vulkan 1.3; Metal renderer configuration | Bundled QEMU version, EL2/nested KVM, actual accelerated render node, native WSI and shared-memory extensions together |
| Lima 2.3 + QEMU 11.1 or later | HVF nested virtualization | A macOS QEMU build with the required GPU/Venus renderer; installing a generic QEMU package alone does not establish this |
| Lima + krunkit | Mesa Venus Vulkan backed by the Mac GPU | Nested KVM and selected guest kernel/page size are unverified; initially useful as a separate native Linux graphics-provider reference |

UTM's current guest documentation requires Default/ANGLE Metal rendering and an
enabled Vulkan driver. Its Apple Core OpenGL backend has no Vulkan support.
The same documentation currently recommends VirGL for the Linux desktop because
the Venus desktop path has missing indirect geometry-shader drawing support.
Older UTM graphics architecture notes describe Vulkan as future work; use the
current versioned release/guest instructions and inspect the actual binary.

Sources: [UTM Linux graphics](https://docs.getutm.app/guest-support/linux/),
[UTM 5.0.9 release](https://github.com/utmapp/UTM/releases/tag/v5.0.9-2),
[Lima 2.3 release](https://github.com/lima-vm/lima/releases/tag/v2.3.0),
[Lima krunkit](https://lima-vm.io/docs/config/vmtype/krunkit/).

## Initial resource budget

Use a separate minimal Linux guest, initially two vCPUs and 4 GiB RAM. A sparse
virtual disk's advertised capacity is not its physical consumption. Account for
the downloaded archive, extracted application/image, guest writes and swap.
Skip a full desktop and Rust/build toolchains initially: copy the existing
qualified runner, original SDK and small fixtures over SSH and install only
native Vulkan/display diagnostics required for the experiment. Keep the working
source trees outside the guest's shared writable mounts.

Host free space was about 3.4 GiB during this iteration. Removing this task's
13,569 already-linked Rust codegen object intermediates reclaimed about 5.9 GiB,
leaving about 9.3 GiB immediately after cleanup. Libraries, metadata, executables,
source and validation logs were preserved. This is a point-in-time measurement,
not an allocation promise. Recheck both host and guest before any installation;
reserve headroom for active development and pause large downloads if the budget
does not fit. The existing CPU VM itself still had only about 1.4–1.7 GiB free.

## Qualification before replacing the reference

1. Record the exact VM/renderer versions and guest kernel. Verify a real
   4096-byte guest Linux page and `trames doctor --json --require-kvm`. Require
   `/dev/kvm` and a successful actual vCPU run in the same VM as the renderer.
2. Capture DRM nodes and `vulkaninfo --summary`; select the accelerated Venus/
   Metal-backed device. A software CPU ICD does not establish GPU acceleration.
3. Run native buffer/shader transfers and the existing native XCB/Wayland target
   and pixel tests. Then run the original guest SDK/KVM headless/presentation
   matrix with both transports and one/four workers on that same device.
4. Record external-memory host imports, FD/image imports/exports, modifiers and
   fences individually. Negotiate the coherent fallback where direct sharing is
   unavailable. CPU page size and GPU allocation alignment are separate inputs.
5. Keep the existing VZ reference until the replacement passes CPU state,
   mappings, cancellation and graphics together. A GPU-only VM can qualify
   native provider behavior separately, with its KVM end-to-end gate pending.

The GPU path would include Linux Venus, a VM renderer and Metal translation.
It helps validate APIs and lifetimes, but cannot measure the bare-metal Linux
driver performance ceiling. Hardware video/VA-API/V4L2 and physical Linux GPU
driver coverage remain separate; a Vulkan-capable virtual GPU does not imply
those devices or codecs are available.

Changing the VM also cannot manufacture SVE support absent from the underlying
CPU/KVM combination. Check CPU capabilities independently of graphics. The
current nested KVM reports SVE unavailable; SME has no implemented KVM ABI in
this backend. Do not infer either extension from successful GPU acceleration.
