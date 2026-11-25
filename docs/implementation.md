# Implementation ledger

Updated 2026-11-25. The [accepted product plan](https://github.com/kb5000/trames-ipad/blob/e93f62b9/docs/roadmap/trames-linux-kvm.md)
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

## Delivered second batch: shared engine and foreground sessions

- The shared runtime `Cpu`/`Engine` and hardware memory contracts are connected;
  the runtime pin remains the same clean shared commit, with no runtime fork.
- Canonical memfd pages/ranges, sparse 4 KiB tables, synchronous hardware-alias
  revocation, readiness handoff, COW and storage/table retirement are implemented.
- Task state migrates independently of its worker-local vCPU. Asynchronous EL0
  capture, partial EL1 save completion, restore recovery, kick retirement and
  pending MMIO completion are implemented. Normal SMCCC exits keep a single RUN
  and avoid per-register resets on continuation.
- Foreground static/dynamic ELF and explicit rootfs mounts use the existing
  syscall/process/signal/thread/clock/VFS paths. One/four-vCPU native comparisons,
  real page-fault signal recovery, fork/COW, pipes, TLS/futex threads and running/
  sleeping cancellation are included in the hardware validation batch.

See the [shared-session validation record](validation/session-20261124.md).
These cases establish an execution foundation, not a complete compatibility or
product release. The baseline advertises FP/ASIMD only; exact instruction budgets,
SVE/SME and debugger single-step remain unsupported.

## Delivered third batch: resource contracts and explicit shared-file imports

- Shared images expose tagged Metal/Vulkan/DMA-BUF capabilities, owned fence FDs
  and explicit per-plane geometry. Apple consumers use checked Metal extraction.
- Linux linear images and dedicated Vulkan host-import buffers retain sealed-file
  bytes, real host granules, native leases and one shared byte-borrow domain.
- KVM imports those original bytes through revocable aliases, deduplicates repeated
  file pages and keeps 4 KiB neighbor access independent. There is no shadow copy.
- Both transports pass bidirectional native Vulkan buffer/KVM execution with zero
  coherent transfer scans, plus ordinary native/KVM workload comparisons. Vulkan
  used lavapipe; this establishes API/memory interop, not hardware GPU readiness.

See the [resource validation record](validation/resources-20261125.md).
Presentation and hardware video providers remain open. The fifth batch below
adds software-validated Linux PCM.

## Delivered fourth batch: shared host CLI and Linux terminal sessions

- Independent trames-cli repository, one pinned common source used by Mac, Linux
  and the existing interpreter wrapper; no runtime/CPU/GUI dependency in that library.
- Literal argv/environment, original session/exec clients, raw terminal ownership
  and signal self-pipes replace duplicated host frontend code.
- Linux explicit-rootfs shell/exec, PTY input/size/restoration, cancellation exit
  codes and compatible socket clients are implemented. Named persistent Linux
  environments and native media providers remain open.
- Root-mount cwd rendering and target-musl DNS linking prerequisites are fixed
  in the shared runtime. Original foreground workload comparisons still pass.

See [CLI architecture](architecture/host-cli.md) and
[validation](validation/cli-20261125.md).

## Delivered fifth batch: Linux PCM and native readiness

- Shared guest ALSA profile and unchanged original instructions now reach an
  on-demand native Linux libasound provider; Apple retains its existing provider.
- Explicit playback/capture flags, exact PCM geometry, nonblocking samples,
  controls/status and native error propagation are connected across all callers.
- Existing idle executors drive ALSA descriptor readiness into the original guest
  eventfd. Controls/close retire descriptor registrations, including FD reuse and
  multiple streams on one endpoint. No Trames service thread or timer is added.
- FFI signatures/constants are compiled against real ALSA headers in CI; the
  checks do not execute in the product. Software PCM and KVM/native comparisons
  are recorded separately from still-pending physical sound/clock acceptance.

See [PCM architecture](architecture/audio.md) and
[validation](validation/audio-20261125.md).

## Delivered sixth batch: shared application acceptance

- A shared manifest and immutable software/rootfs bundle drive native reference
  and KVM SMCCC/MMIO at one/four workers, with actual workflows rather than
  version probes. Build receipts and independent evidence auditing bind results
  to real source/artifact/configuration inputs. JSON/JUnit and a coverage table
  distinguish failures, missing prerequisites, invalid references and unrun work.
- Passing evidence can be reused only with matching inputs and intact raw logs/
  output files; failed cases run again. Native-reference CI and a separate real
  KVM application workflow are connected without production runtime changes.
- The initial baseline was native 12/12 and KVM 7/48. Subsequent shared-file
  and original JIT fixes, followed by batch seven below, pass all 48 KVM
  application cells. Historical failures remain available; these scoped
  workflows do not establish arbitrary software support.

See [acceptance design](architecture/acceptance.md) and
[first evidence](validation/applications-20261125.md).

## Delivered seventh batch: IP, systemd and Docker lifecycles

- Shared ICMP filtering/timestamps/error queues, keyring graph/buffer behavior,
  namespace-relative pidfd fdinfo, ramfs/debugfs and overlay inode retirement
  are implemented across runtime callers, with native comparisons.
- KVM caches bounded per-mm roots, isolates non-global translations with
  per-worker/per-mm ASIDs and preserves completed notifications across preemption.
  Hardware regressions cover register state, original code at overlapping VAs,
  eviction/retirement and both configured ASID widths. ABI checks stay compiled;
  normal SVCs add no register-reset ioctl, RUN or coordinator thread.
- Clean qualification passes CLI 48/48, IP 8/8, systemd 4/4 and Docker 4/4,
  with 80 real containerd shim lifecycles across both transports and 1/4 workers.
  The shim gate uses Connect/Shutdown RPCs to avoid the upstream SIGTERM startup
  race. Native references and relocated evidence are independently audited.
  Exact passing cells are reused; changed/failed cases execute again.

See the [implementation, qualification and limits](validation/ip-systemd-docker-20261125.md).
Docker still reports unsupported swap/IO limits. These are scoped service
workflows, not full systemd/container/cgroup compatibility or a release.

## Next dependency batch: Linux providers

1. Complete Linux Vulkan image imports/exports, presentation, video and PCM hardware validation as detailed
   below. Keep Apple consumers working and keep headless execution independent
   of device initialization. Probe real hardware before claiming capability.
2. Expand runtime compatibility, signal/restart, exec, shared mappings, resource
   pressure, terminal/stdio and teardown validation in complete dependency batches.
   Add debugger single-step and only advertise extended CPU state after support.
3. Measure bare-metal distributions, CPU time, asynchronous register capture,
   mapping synchronization and ordinary workloads. Optimize observed costs with
   the shared ownership rules; nested-VM timing cannot predict bare-metal limits.

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
- The existing guest PCM front end now has a native ALSA provider for playback
  and capture. Complete real device poll/xrun/device-loss recovery, latency,
  clock calibration and A/V synchronization. PipeWire currently uses host ALSA
  plugins; direct PipeWire support remains a separate provider choice.

GPU alignment must remain independent of CPU pages. Validate API buffers,
image import/export and fallbacks with real 4096-byte host/guest pages. A host
without hardware reports its actual capability; it cannot replace required
graphics/audio/video acceptance. A GPU/media-capable ARM64 Linux machine is
still needed for those tests.

## Subsequent gates

- Finish the remaining accepted L0–L2 and broader compatibility/resource gates;
  the recorded fixtures cannot claim untested corpus or application passes.
- Complete real application graphics/audio/video, native comparisons, resources,
  performance and packaging before the ARM64 product release.
- Add amd64 as a separate guest-ABI/runtime and KVM architecture batch. Common
  Linux modules must not grow ARM64 register or syscall assumptions. Building
  this workspace on amd64 only validates portability of the current skeleton.

Keep one development line on `main`, publish validated batches promptly, and
advance the runtime pin only to a real shared commit. Preserve ongoing runtime
iterations and reuse unaffected passing validation evidence.
