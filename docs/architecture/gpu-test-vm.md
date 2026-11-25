# Mac-hosted Linux GPU test VM

Status, 2026-11-25: **installed and verified as a native Linux GPU reference**.
UTM 5.0.9 / QEMU 10.0.12 provides accelerated Venus/Metal graphics. The existing
Lima VZ VM remains the ARM64/4 KiB/nested-KVM reference. Current UTM has no
`/dev/kvm`; Trames doctor with `--require-kvm` exits 2. GPU and KVM have not yet
been qualified together in one VM.

| Item | Actual configuration |
| --- | --- |
| UTM name | `Trames Linux GPU` |
| Guest | Ubuntu 24.04.5 LTS, ARM64, kernel `6.8.0-146-generic` |
| Pages | Real 4096-byte Linux pages |
| Resources | Two vCPUs, 4 GiB RAM, 16 GiB sparse disk |
| Vulkan | Mesa 25.2.8, `Virtio-GPU Venus (Apple M6)` |
| OpenGL | Accelerated VirGL through the Metal renderer |
| Display | Weston 13 on DRM/GL, seatd and Xwayland; starts on guest boot |
| Access | Local SSH port 50222, user `trames`, dedicated local identity |

The cached minimal Ubuntu image was reused as an APFS clone. Its new disk has
about 1.8 GiB allocated blocks, including blocks shared with that base. Copied
executables and Mesa packages provide testing without a Rust toolchain or full
DE. The VM has its own disk and no shared writable source mount. UTM's official
installer hash, Apple signature and notarization passed; the installer was then
removed. See the [qualification and disk record](../validation/gpu-vm-20261125.md).

## Use

Open UTM and select **Trames Linux GPU**, or:

```sh
/Applications/UTM.app/Contents/MacOS/utmctl start 'Trames Linux GPU'
ssh -F ~/dev/trames/target/local-ci/utm-gpu/ssh_config trames-gpu
# Inside the guest: explicitly select the accelerated ICD.
trames-gpu-env vulkaninfo --summary
trames-gpu-env vkcube-wayland --c 60
```

For normal shutdown:

```sh
/Applications/UTM.app/Contents/MacOS/utmctl stop --request 'Trames Linux GPU'
```

UTM CLI's unqualified `stop` defaults to immediate power-off. Use `--request`
or guest `sudo poweroff` to save pending writes. Normal shutdown/restart, SSH,
desktop startup and all three GPU cases were rechecked. QEMU debug logs are off;
guest journals are bounded to 64 MiB on disk and 16 MiB in memory. The SSH
identity stays in the local workspace.

## Coverage and remaining gate

Forced-Venus tests pass for a 4096-byte GPU buffer fill/readback, a Wayland Vulkan
window and original XCB Vulkan rendering with every RGB pixel checked through
Xwayland. All three pass after normal shutdown/restart. Accelerated OpenGL and
a real Vulkan-window screenshot are recorded. External-memory host imports,
DMA-BUF images/modifiers/fences, video devices and codecs need separate coverage.

The installed QEMU's ARM64 HVF binary lacks the EL2 enable API and the current
guest exposes no KVM. Lima 2.3 documents nested HVF support with QEMU 11.1 or
later. A future build must combine that support with the required Venus/Metal
renderer. A generic QEMU package alone does not establish GPU support. Require
real 4 KiB pages, actual KVM vCPU execution, the CPU-state matrix and original
SDK/KVM presentation cases on the accelerated device before replacing VZ.

Venus/Metal translation is useful for API/lifetime testing. Bare-metal Linux
GPU performance and hardware video remain separate. Graphics virtualization
does not add SVE/SME support to the CPU/KVM contract.

Primary sources: [UTM graphics](https://docs.getutm.app/guest-support/linux/),
[UTM release](https://github.com/utmapp/UTM/releases/tag/v5.0.9-2),
[pinned dependencies](https://github.com/utmapp/UTM/blob/v5.0.9-2/patches/sources),
[Lima 2.3](https://github.com/lima-vm/lima/releases/tag/v2.3.0), and
[Weston](https://wayland.pages.freedesktop.org/weston/toc/running-weston.html).
