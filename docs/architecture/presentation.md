# Linux Vulkan presentation

The Linux product exposes the shared guest Vulkan bridge through native
`libvulkan`. Compute can run without a display. The first foreground presenter
uses an original host X11 window; the shared display contract also accepts an
original, configured Wayland surface. Apple continues to use its Metal target.

```sh
trames run --rootfs /path/to/rootfs --cpus 4 \
  --vulkan-icd /path/to/libtrames_vulkan_icd.so \
  --display x11 --display-size 1024x768 \
  --setenv VK_DRIVER_FILES=/usr/share/vulkan/icd.d/trames.json \
  -- /path/to/guest-vulkan-program
```

`--vulkan-icd` installs the existing guest SDK and its manifest into the current
rootfs/mount namespace and enables the GPU context. Supply an original SDK built
for the guest architecture and libc. `--vulkan` enables an already installed SDK;
omit `--display` for headless execution. `--display-size` accepts dimensions from
1 through 4096. Installing the SDK writes the explicitly selected environment.
SDK packaging and cross-libc builds remain separate work.

## Native targets and ownership

The host supplies a typed Metal, XCB or Wayland target, its dimensions and an
owning lease. Guest handles cannot supply host window pointers. The Vulkan
instance enables `VK_KHR_surface` and the target's actual platform extension
only after confirming native support. Surface creation rejects a target of a
different platform from that enabled on the original instance.

The surface retains the original connection/window owner through swapchain
teardown, including after the embedding detaches its display. A resized target
retains the live surface claim, so the new target generation cannot acquire a
second producer over an existing surface. Native presentation uses the driver's
swapchain; the product does not read back pixels or copy rendered frames into a
host image. The pixel readback in acceptance belongs to the external test probe.

The foreground adapter opens separate XCB event and Vulkan connections. This
keeps driver requests from consuming or buffering host input readiness. The
existing foreground `HostWait` watches the event descriptor alongside signals
and the actual session deadline. Configure events update the target dimensions;
`WM_DELETE_WINDOW` and window destruction cancel the original session. There is
no additional Trames window thread, periodic retry or service bus.

The public Wayland target is a native WSI capability, with a tested retained
connection/surface lifetime. This batch does not add a Wayland foreground shell
or a guest Wayland/X11 desktop compositor. Applications currently present through
the shared `VK_KHR_display` fixed-display interface in the X11 foreground window.
Keyboard/pointer/clipboard integration and full desktop presentation need their
own product adapter.

## CPU and GPU memory contracts

`Cpu::NATIVE_POINTER_IMPORTS` declares at compilation whether the execution
backend can import a driver-owned pointer. KVM declares `false`: a pointer alone
does not provide the separately revocable alias required by its memory contract.
The shared session passes that capability when creating a GPU context; additional
original instance/device graphs inherit it. Ordinary guest execution gains no
capability probe or extra instruction/memory check.

Dedicated sealed-file Vulkan buffers keep direct sharing of their original
bytes through KVM's FD importer. Generic driver allocations use the existing
coherent transfer path on the original worker. This preserves their allocation
and alias semantics without trying to import an unsupported raw pointer or
forcing a structural checkpoint for it. It does not make all Vulkan mappings
zero-copy. GPU allocation alignment remains independent of CPU page size.

The SDK installer resolves paths through the caller's VFS namespace, including
host mounts, chroots and directory aliases. It writes a bounded temporary sibling,
syncs/closes it and renames it into place. Identical installed bytes retain their
inode. The same namespace correction covers the existing Zink, VA-API and codec
install helpers.

## Acceptance and remaining dependencies

[`validate-wsi.py`](../../scripts/validate-wsi.py) compares original native and
guest Vulkan programs, reads actual X11 server pixels, tests two swapchain
lifecycles, keeps stdin open during host-close cancellation, and verifies native
XCB/Wayland owner retention. Headless tests use invalid display names and multiple
original instances/devices. KVM runs both SMCCC and MMIO with one/four workers.

Hosted CI runs the native Mesa/Xvfb/Weston references on Linux ARM64 and AMD64.
Those jobs make no KVM or physical GPU claim. Local 4 KiB ARM64 KVM results and
exact build inputs are recorded in [the validation report](../validation/presentation-20261125.md).

Remaining dependencies include authenticated DMA-BUF image import/export and
sync fences, guest Wayland/Xwayland/Zink integration, native Wayland foreground
events, hardware rendering/display, VA-API/V4L2 video and physical PCM/A/V clock
acceptance. No physical GPU is exposed by the current nested VM.

Native API contracts: [Khronos XCB surface](https://docs.vulkan.org/refpages/latest/refpages/source/vkCreateXcbSurfaceKHR.html),
[Wayland surface](https://docs.vulkan.org/refpages/latest/refpages/source/vkCreateWaylandSurfaceKHR.html)
and [WSI ownership/order requirements](https://docs.vulkan.org/spec/latest/chapters/VK_KHR_surface/wsi.html).
