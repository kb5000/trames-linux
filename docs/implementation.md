# Implementation ledger

Updated 2026-11-24. The [accepted product plan](https://github.com/kb5000/trames-ipad/blob/e93f62b9/docs/roadmap/trames-linux-kvm.md)
defines the full scope. This ledger tracks implementation, without changing its
graphics/audio/video or complete 4 KiB release requirements.

## Delivered first batch: product skeleton and KVM transport

- Independent repository, explicit shared-runtime commit, locked dependencies,
  Linux/Apple build checks and a separate required-hardware workflow.
- Linux ARM64 KVM execution of original EL0 instructions, with private EL1
  save/restore and a runtime-layout mailbox; SMCCC and MMIO notifications.
- One bounded RAM slot, nonidentity VA/GPA translation, real 4 KiB mappings,
  canonical byte access, single-page protection/fault repair, and an unmapped hole.
- Baseline GPR/FP/SIMD/TLS transfer, EL0 mailbox access protection, and a
  thread-targeted deadline that interrupts a syscall-free loop.

Both transports passed on the existing nested ARM64 Linux debug VM. See the
[validation record](validation/l0-20261124.md). This completes the first transport
prototype, not all L0 gates. The VM has no GPU, desktop session, PCM device or
video device; it cannot establish hardware media readiness.

## Next dependency batch: shared engine, memory and asynchronous exits

Implement the complete dependency set before consolidated validation:

1. Connect `HardwareStorage`/`HardwareRange` to canonical runtime backing. Keep
   stable arena registration bounded; handle map/unmap/protect, borrow/revoke,
   COW, reclaim and reuse with the existing ownership rules. Verify adjacent
   4 KiB pages remain independent throughout; do not introduce a second byte store.
2. Implement the runtime `Cpu`/worker-local `Engine` contract. Normalize syscall,
   page fault, invalid instruction, breakpoint and native bridge entries. Use
   shared Linux dispatch, ELF loading, signal frames, memory and resource objects.
3. Complete state capture at asynchronous exits, including partial EL1
   save/restore and pending MMIO/hypercall completion before parking/migration.
   Add external interrupt lifetime/entry gates and worker handoff. Blocking work
   returns its execution slot; short calls stay on the current worker.
4. Audit the real KVM CPU profile, HWCAP/auxv and unsupported instruction state.
   Baseline L0 does not advertise SVE/SME or promise exact instruction counts.
5. Add foreground ELF/rootfs CLI execution and explicit headless capability
   reporting. Validate static ELF, fault/signal/TLS/FP, mmap/COW/futex and worker
   handoff in the actual 4 KiB VM; compare affected cases to native Linux.

## Shared API and Linux providers

The common resource contract is a dependency of the first graphical/media
delivery, alongside the shared-engine work:

- Replace mandatory Metal texture exposure in shared images and all callers
  with platform-neutral image metadata, native handles, FD ownership, import
  capability, lifetime and synchronization. Preserve existing Apple consumers.
- Reuse the guest Vulkan API bridge with host libvulkan. GL/GLES/EGL use Mesa
  Zink. Reuse the shared Wayland compositor with Linux Wayland/X11 presentation.
- Implement libva/VA-API and selected SoC V4L2 providers with negotiated structured
  VA/bitstream modes and DMA-BUF import. Validate H.264 hardware decode/display;
  validate the selected encode path separately.
- Implement the existing ALSA guest PCM front end with PipeWire/ALSA playback
  and capture, poll/xrun/device loss, timing and A/V synchronization.

GPU alignment must remain independent of CPU pages. Validate API buffers,
image import/export and fallbacks with real 4096-byte host/guest pages. A host
without hardware reports its actual capability; it cannot replace required
graphics/audio/video acceptance. A GPU/media-capable ARM64 Linux machine is
still needed for those tests.

## Subsequent gates

- Finish the accepted L0–L2 ownership, scheduler, process and compatibility gates;
  the current probe cannot claim those passes.
- Complete real application graphics/audio/video, native comparisons, resources,
  performance and packaging before the ARM64 product release.
- Add amd64 as a separate guest-ABI/runtime and KVM architecture batch. Common
  Linux modules must not grow ARM64 register or syscall assumptions. Building
  this workspace on amd64 only validates portability of the current skeleton.

Keep one development line on `main`, publish validated batches promptly, and
advance the runtime pin only to a real shared commit. Preserve ongoing runtime
iterations and reuse unaffected passing validation evidence.
