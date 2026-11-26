# Linux desktop interoperability — 2026-11-26

The Linux frontend now provides host X11 and native host Wayland windows,
bidirectional text clipboard, host input methods and Vulkan desktop composition.
It uses the shared Runtime's existing compositor, input/text interfaces and
userspace kernel. KVM remains the CPU/MMU transport and no loaded guest
instructions are modified.

The final runner is SHA-256
`747c462ef1d31888347949b4c874caa1b5182c8e8e7cf2a954fb26bb2b36904f`,
paired with real Runtime commit
[`ef0b919`](https://github.com/project-trames/trames-runtime/commit/ef0b9194a84ba10ad0952e84411d3943d0f18452).
Its [build receipt](desktop-interop-20261126/final-build.json) records the actual
ARM64 Linux compiler, clean Runtime source identity and dirty-but-frozen product
source identity before this delivery commit. The offline auditor can compare
that input hash directly with the checked-out committed sources. Documentation
and workflow metadata are outside the executable source fingerprint.

## Results

All 49 desktop cells pass on the final frozen binary:

| Gate | Passes | Coverage |
| --- | ---: | --- |
| Original native references and frontend interop | 8/8 | Native Vulkan X11/Wayland reference pixels; X11 clipboard/XIM; Wayland copy, paste, IME; Vulkan composition of SHM on both hosts |
| Real ARM64 KVM interop | 24/24 | SMCCC/MMIO × 1/4 vCPUs × X11 clipboard/XIM, Wayland copy/paste/IME, Vulkan windows on X11/Wayland |
| Original guest Xwayland/Xlib IME | 4/4 | Host XIM → native frontend → shared guest XIM → original Xlib Unicode commit, both transports and CPU counts |
| Existing desktop regression | 13/13 | Original Wayland/Xwayland clients, keyboard/pointer/wheel/focus, resize/expose/close, idle CPU, managed-store create/reopen, Weston Terminal, xterm, CLI continuation |

[Interop results](desktop-interop-20261126/qualification-final/results.json),
[Xwayland IME results](desktop-interop-20261126/xwayland-ime-final/results.json)
and [desktop regressions](desktop-interop-20261126/desktop-regression-final/results.json)
retain their own exact case sets, command lines and raw client logs. Each matrix
freezes its executable copies before running; case-specific scratch rootfs trees
are isolated and removed after their input hashes have been recorded.

Shared library regressions pass **97** tests: graphics 3, Vulkan 67, Wayland 27.
One pre-existing native Vulkan test remains explicitly ignored. Product tests
pass **23**; hardware/native-protocol tests marked ignored are not included in
that unit-test count. Linux all-target clippy, Apple compilation/all-target
clippy, formatting, real-pin boundaries and reviewed notices pass. The three
new dependency records and their original license texts are included in the
139-package locked dependency inventory.

The host uses 4096-byte pages, original Xwayland 23.2.6 and Weston 13.0.0. GPU
references select Mesa software Vulkan. There is **no `/dev/dri` device** in this
VM: these results qualify interoperability and correctness, not physical GPU
acceleration, comparative performance or Vulkan conformance. Existing CPU,
memory and command-line workload qualification remains at its recorded source
cohort; it is not relabelled as a new result for this desktop change.

## Implementation and corrected cases

Clipboard exports/imports use the existing `HostClipboard` capability with
nonblocking pipes and bounded transfers on the original foreground `HostWait`.
X11 supports text target negotiation, Latin-1 fallback and incoming INCR;
Wayland uses real selection sources/offers and suppresses its own exported
selection echo. Host text follows the shared API's 16 KiB limit.

Host X11 connects to its XIM server; host Wayland uses text-input-v3 with v1
fallback. Preedit, UTF-8 cursor positions, surrounding deletion and confirmed
Unicode text retain the shared context/epoch/serial contract. Weston desktop
shell supplies the original input-method-v1 provider used by the native
Wayland tests. Its kiosk shell is used separately for deterministic pixels.
The extra Xlib client uses standard input-method instantiate callbacks when
Xwayland's socket becomes ready before the shared XIM service registers.

Vulkan composition samples retained GPU images on their original device/queue,
with no CPU pixel readback. SHM planes upload only when Vulkan composition is
selected. Device leases are separate from source image-slot leases; every GPU
case renders sixteen frames over two swapchain generations and tears down
successfully. Source images remain retained through the presenter's GPU fence.
Only actual shared queue operations synchronize. There is no per-frame
device-wide idle, new frontend service thread or CPU execution ABI check.

The first software-ICD presentation attempt showed pixels but received no
implicit XPresent completion event. The final X11 frontend explicitly requests
an MSC notification after ordering the driver's native requests, so callbacks
work across both software and native WSI paths. Native Wayland uses host frame
callbacks and a prepared read across HostWait, cancelled before native WSI
operations; buffered UI events latch the same existing wake. Teardown drops
renderers before their native windows and connections.

Current scenes must share one native Vulkan device/queue for their GPU planes;
separate windows may use separate devices. Full physical keyboard layout
negotiation, cross-device/mixed-queue composition, general external DMA-BUF
synchronization and physical GPU/video acceptance remain subsequent work.
These boundaries are also described in the [desktop guide](../architecture/desktop.md).

## Reproduction and audit

Install native Vulkan, XCB/XTEST, Wayland development/protocol tools, Xvfb and
Weston. For the extra original Xlib gate, provide Xlib development headers and a
rootfs/launcher produced by the existing desktop controller. Build the original
guest Vulkan ICD for the test libc; its unmodified source, header revision and
build command are recorded in [guest-icd-source.json](desktop-interop-20261126/guest-icd-source.json).

```sh
VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json \
  python3 scripts/validate-desktop-interop.py \
  --unit-binary UNIT --runner TRAMES --guest-icd ICD --output NEW_DIRECTORY
python3 scripts/validate-xwayland-ime.py \
  --unit-binary UNIT --runner TRAMES --rootfs DESKTOP_ROOTFS \
  --launcher LAUNCHER --output NEW_XIM_DIRECTORY
python3 docs/validation/desktop-interop-20261126/audit.py --source-root "$PWD"
```

The auditor verifies archived hashes, exact 49-cell coverage, successful original
client output, Unicode transfers/commits, frame generations, 4 KiB KVM doctor
output, shared Runtime pin, compiler and executable identity, 97/23 unit counts
and optional full source/build fingerprint matching. About 1.6 MB of evidence
is archived; large binaries, rootfs/store trees and downloaded development
packages are excluded. The existing VM/shared build cache is reused; no VM disk
image is created.

ARM64/AMD64 hosted presentation CI now runs all eight native interop references
in addition to the existing WSI/desktop gates. These jobs are explicitly native
software references, not KVM or physical GPU qualification. Cloud run identities
and exact status are maintained in [cloud-checks.json](desktop-interop-20261126/cloud-checks.json).
