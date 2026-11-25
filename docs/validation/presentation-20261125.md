# Linux Vulkan presentation qualification — 2026-11-25

The Linux foreground now presents the shared fixed-display Vulkan API in a real
host X11 window. Headless Vulkan runs independently. Shared native targets retain
their Metal/XCB/Wayland owners, and the guest SDK installer follows the selected
rootfs namespace. This is the eighth implementation batch, following the
[IP/systemd/Docker qualification](ip-systemd-docker-20261125.md).

## Frozen inputs

- Linux source: `273678318b86355978657d04a66deee050a05491`, clean.
- Shared runtime: `ff906ed3f4caf1d33b3f3f9dd1cf423a6d78d646`, clean.
- Release runner SHA-256:
  `40a3cb218d51781542fa4f82389a3363e6a76eb11260948c24445ac68c76a7a8`.
- Original, unchanged guest SDK sources: iPad `d61ac3d3273a93946f71b05e069c66c9acf87181`,
  22 source hashes in [the source record](presentation-20261125/guest-icd-source.json).
  The SDK was compiled against the VM's GNU libc and the existing pinned
  Vulkan-Headers revision. No guest application/library instructions were patched.
- ARM64 Linux nested KVM on the existing Mac debugging VM; doctor confirms real
  4096-byte host pages, API 12, SMCCC forwarding and no capability issues.
  Mesa lavapipe, private Xvfb and private Weston provide software references.
  The controller terminates its test compositors; it installs no persistent service.

The [build receipt](presentation-20261125/trames-wsi-qualified-build-final.json),
[case inputs](presentation-20261125/trames-wsi-qualification-final/inputs.json),
[doctor output](presentation-20261125/trames-wsi-qualification-final/doctor.stdout),
and [environment/driver identities](presentation-20261125/environment.json) retain
the toolchain, hashes and configuration. Later commits change no qualified
production or fixture source. The exact controller used for the 13-cell KVM
matrix is also [archived](presentation-20261125/qualified-controller.py); the later
CI socket-directory correction changes test environment setup only.

## Results

| Scope | Passed | What ran |
| --- | ---: | --- |
| Native target lifetime group | 1/1 | Two real Rust tests: X11 resize/close/claim retention and Wayland surface/connection retention |
| Native Vulkan reference | 2/2 | 16 presented frames/two swapchains with actual X11 pixels, plus headless buffer validation across multiple instances/devices |
| KVM headless | 4/4 | SMCCC/MMIO × one/four workers, invalid display names, original generic Vulkan allocations |
| KVM presentation | 4/4 | Same matrix; 16 frames/two swapchains and actual 320×240 green pixels in each host window |
| KVM host close | 2/2 | Both transports/four workers, original guest blocked on an open stdin pipe, real `WM_DELETE_WINDOW`, exit 0 |
| **Final matrix** | **13/13** | No omitted or failed cells |
| Dedicated Vulkan FD buffers | 2/2 | Both KVM transports; original bytes visible in both directions, zero coherent-scan metrics |
| Shared Linux unit suites | 107 | Distro 40; Vulkan 67; the latter retains one separately gated test |
| Shared Mac unit suites | 106 | Distro 40; Vulkan 66; 14 platform/device tests retain their existing gates |
| Product workspace unit suite | 12 | Ordinary tests; 18 explicit hardware/display gates, with the two new display tests executed above |

All relevant strict clippy checks pass on Linux and Mac. The product format and
architecture-boundary checks pass. The Mac workspace/all-target check and iOS
ARM64 `trames-ffi` check pass with the new shared runtime. Unaffected CPU, network,
PCM and service evidence from earlier batches is reused rather than rerun.
This batch does not refresh those workflows against a new arbitrary software corpus.

The pixel probe reads every RGB pixel from the original X server and asserts
the expected color/geometry. `pixels_sha256` in the case record hashes its saved
`pixels.stdout` transcript; it is not a raw screenshot hash. The probe/controller
source hashes are bound in `inputs.json`.

Raw [matrix results](presentation-20261125/trames-wsi-qualification-final/results.json),
[native target log](presentation-20261125/trames-wsi-qualification-final/native-targets.log),
[workspace tests](presentation-20261125/trames-wsi-final-unit.log),
[Linux shared tests](presentation-20261125/trames-wsi-runtime-tests.log),
[final Vulkan tests](presentation-20261125/trames-wsi-mapping-runtime-tests.log),
[SMCCC FD check](presentation-20261125/trames-wsi-dedicated-smccc.log),
[MMIO FD check](presentation-20261125/trames-wsi-dedicated-mmio.log),
[Mac check](presentation-20261125/mac-product-check.log) and
[iOS check](presentation-20261125/ios-arm64-check.log) are retained with the other
individual stdout/stderr/build/check logs.

## Failures closed by this batch

Historical runs remain in the evidence directory:

1. Xvfb reset when the target tests disconnected, invalidating the following
   native reference. The private test server now uses `-noreset`.
2. SDK installation into a mounted/chroot rootfs failed with errno 2 because the
   private import interface bypassed the namespace. The shared installer now
   creates/syncs/renames through the VFS namespace; the regression covers a real
   host mount, chroot, `/usr` directory alias and unchanged user files/inodes.
3. Generic Vulkan MapMemory attempted to import a pointer without a KVM alias
   contract and failed with `BackingIo(95)`. KVM now declares its pointer-import
   capability at compile time; original generic allocations use the existing
   coherent path and dedicated file-backed buffers retain direct sharing.
   Additional original instance/device graphs inherit the policy.
4. The close controller first closed stdin too early, then exposed an X11 sender
   that disconnected without confirming message delivery. It now keeps stdin open
   until cancellation completes and acknowledges the checked X11 request. Both
   native event handling and end-to-end KVM close pass; no runtime exit workaround
   or new polling/service thread was introduced.
5. The first hosted ARM64 WSI job compiled successfully but Weston could not
   start: the checkout/evidence directory made its socket path exceed Unix's
   108-byte address limit. The controller now uses an owned, mode-0700 temporary
   directory under `/tmp`, independent of the evidence path, and cleans it after
   both compositors stop. The original
   [failure](presentation-20261125/ci-arm64-initial-failure/weston.log) remains
   separate from passing native and KVM results.

The pre-fix 11/13 run is preserved as
[`trames-wsi-qualification-v4`](presentation-20261125/trames-wsi-qualification-v4/results.json).
The final pass uses a fresh clean build receipt and frozen runner. Earlier runs
and diagnostic logs remain separate and never count as final passing cells.

## Reproduction and offline audit

Install native Vulkan/XCB/Wayland headers, `wayland-scanner`, Xvfb and Weston.
Build the unchanged guest SDK for the test rootfs libc, then create a runner
receipt with `runtime/tests/acceptance/build.py`. Build the ignored native target
unit binary with `cargo test -p trames-linux-app --release --no-run --message-format=json`.
Use the executable emitted for the binary test target:

```sh
VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json \
  python3 scripts/validate-wsi.py \
  --runner /path/to/frozen/trames --receipt /path/to/build.json \
  --guest-icd /path/to/libtrames_vulkan_icd.so \
  --unit-binary /path/to/original/unit-binary --output /new/evidence/directory
python3 docs/validation/presentation-20261125/audit.py
```

The offline audit checks every recorded file hash, exact case set/status, clean
build inputs, fixture and archived-controller identity, 4 KiB doctor data, unit artifact
identity, native pixel transcripts and guest output. It works from a relocated
Git archive and needs neither KVM nor compiled artifacts. Large rootfs/binaries
are excluded from this committed evidence. Hosted ARM64/AMD64 CI uses
`--native-only` for the three software reference cells and makes no KVM claim.

## Remaining acceptance

There is no physical `/dev/dri` device in this VM. These results qualify software
WSI and the tested 4 KiB CPU/GPU bridge paths; they do not qualify physical GPU
rendering, DMA-BUF/fence interchange, hardware video or native performance.
The product frontend is X11 fixed-display Vulkan. The retained native Wayland
target has real surface-lifetime acceptance, but a Wayland frontend, guest
Wayland/Xwayland desktop, Zink/GLES and keyboard/pointer/clipboard remain further
integration work. SDK packaging for multiple libcs, physical PCM/A/V clock tests,
bare-metal performance and amd64 guest execution remain release gates.

See [the implemented resource and host-loop design](../architecture/presentation.md).
