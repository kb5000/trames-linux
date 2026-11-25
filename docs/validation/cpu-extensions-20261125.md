# ARM64 CPU extensions, FP state and single-step — 2026-11-25

This ninth implementation batch exposes the actual initialized KVM vCPU's
supported instruction features through shared auxv/cpuinfo, adds architectural
single-step without changing original code, and preserves extended FPCR/FPSR
state across signals and ptrace. The [architecture](../architecture/cpu-capabilities.md)
describes the cold discovery path and remaining stateful extensions.

## Frozen qualification inputs

- Linux source: `bd98544344bfe4c5c177de9818f5e0992c9df101`, clean.
- Shared runtime: `474ee466391e2683c1a0f48e6fb21307cc45adae`, clean.
- Frozen release runner SHA-256:
  `44f1f61e8e3db16e57af99f128d8a4be8566a81894cb1bf1697ac13a5e370834`.
- Existing Mac-hosted ARM64 Linux nested-KVM VM, real 4096-byte pages.
  The actual KVM profile exposes 32 feature names, HWCAP `783546363` and HWCAP2
  `562954251559297`. SVE and MTE are unavailable. KVM reports PAuth availability,
  but the backend does not enable/advertise it without its complete task ABI.

The [build receipt](cpu-extensions-20261125/trames-cpu-extensions-build-v5.json),
[bound fixture/controller/profile inputs](cpu-extensions-20261125/trames-cpu-extensions-validation-v5/inputs.json),
[doctor output](cpu-extensions-20261125/trames-cpu-extensions-validation-v5/doctor.stdout)
and [raw results](cpu-extensions-20261125/trames-cpu-extensions-validation-v5/results.json)
retain source identities, toolchain, binary and fixture hashes. Later documentation
commits change none of these qualified production, fixture or controller inputs.

## Results and evidence reuse

| Check | Result | Scope |
| --- | --- | --- |
| Native/SMCCC/MMIO CPU matrix | **15/15** | Three native references; original instructions, four-thread FP/signal state and ptrace at one/four workers with both transports |
| Mac HVF cross-check | **4/4** | Same original ELF; FP/signal state and ptrace at one/four workers with final shared runtime |
| Final Linux shared tests | **108** | 106 kernel tests and two task-signal tests, including extended controls |
| Final Mac shared tests | **108** | Same affected packages; iOS ARM64 FFI check also passes |
| KVM library/hardware checks | **46** reused | Both transports, 16 library + seven real hardware cases each, before the shared FP-mask correction; executor/hardware test sources unchanged |
| Declared CLI workflows | **48/48 KVM**, 12 native reused | Reexecuted after dynamic HWCAP/single-step changes; exact prior native reference reused; before the later extended FP-mask correction |

The last two rows bind to runtime `f767b2f56dba9761f4280569b358bd3855fc69b7`
and, for CLI, Linux `b391de87a5d1df7da4a21f2218392f5bf269ceee`. Their receipts
remain distinct. The final shared correction only removes obsolete user FP
register masks and extends their regressions; the final 15-cell matrix and HVF
checks explicitly reexecute both changed signal and ptrace paths with AFP/EBF16
controls. Normal CLI workflows do not set these extension controls. Earlier
network/media/presentation evidence remains at its original scope and identity;
this batch does not requalify physical devices or complete 4 KiB media operation.

The CPU fixture checks representative actual extension instructions against
native byte/status results, exact advertised flags, 256 signal frames across
four threads, and GET/SETREGSET floating-point controls across debug stops.
Native arm64's original `ADD; ADD; SVC(getpid); ADD` sequence produces five stops:
PC offsets `4,8,12,12,16`, with `si_code` values `2,2,0,2,2`. Both KVM transports
and Mac HVF now match this behavior. No application/library instruction is patched.
The signal-return and ptrace masks had independently discarded newer FPCR bits;
both now preserve the complete Linux user registers.

Linux and Mac strict affected clippy checks pass; product format and boundary
checks pass. The [final Linux tests](cpu-extensions-20261125/trames-cpu-extensions-final-kernel-session.log),
[Linux clippy](cpu-extensions-20261125/trames-cpu-extensions-final-v5-clippy.log),
[Mac tests](cpu-extensions-20261125/mac-final-kernel-session.log),
[Mac clippy](cpu-extensions-20261125/mac-final-shared-clippy.log),
[iOS check](cpu-extensions-20261125/mac-final-ios-check.log),
[HVF results](cpu-extensions-20261125/apple-final/results.json),
[SMCCC hardware log](cpu-extensions-20261125/trames-cpu-extensions-hardware-smccc.log),
[MMIO hardware log](cpu-extensions-20261125/trames-cpu-extensions-hardware-mmio.log)
and [CLI report](cpu-extensions-20261125/applications-report/report.md) retain
their original raw records. The [offline audit](cpu-extensions-20261125/audit.py) checks the source/receipt, original raw outputs and retained CLI artifact hashes; its [result](cpu-extensions-20261125/audit.json) passes. It replays recorded Linux checkout group-write permissions without altering source bytes or executable bits. The hardware CI workflow includes the CPU matrix
with a build receipt; a hosted compile cannot stand in for its real KVM gate.

## Historical failures retained

- V1 enabled debug MDSCR in ordinary execution and caused SIGTRAP exits. The
  corrected executor isolates debug mode and retains the normal entry path.
- V1/V2 fixture expectations assumed every debug stop was TRAP_TRACE and then
  assumed only four stops. The native Linux oracle established the code-0 syscall
  stop and pending trace stop; shared kernel semantics were corrected accordingly.
- V3 passed the original baseline FP controls. V4 added AFP/EBF16 across signals:
  all native cases passed, but all four KVM signal-state cells failed. The final
  shared mask correction and additional ptrace controls pass V5 **15/15**.

All four historical result directories and the final V5 outputs are retained
separately. Failures have not been relabeled or replaced by weaker expectations.

## Integration, limits and disk budget

Validated commit identities are preserved on main. Mac and iPad runtime pins are
recorded in [the paired-pin record](cpu-extensions-20261125/cpu-consumer-pins.json).
The owner's concurrent runtime work had moved FP restore to another file. The
integration preserves that rewrite and ports only the same two mask removals to
its new location. All other 270 working files were preserved by byte hashes; see
[the integration record](cpu-extensions-20261125/runtime-cpu-integration.json).
That uncommitted owner work is outside the frozen qualification above.

SVE/SME state, PAuth/MTE task state and exact instruction budgets remain pending.
This record establishes functional agreement for the declared cases; it makes
no native-speed performance claim. GPU/codec hardware qualification is separate.
The [GPU VM assessment](../architecture/gpu-test-vm.md) identifies candidates and
the GPU-plus-KVM gates required before replacing the existing reference VM.

No VM image or application was downloaded. Cleaning 13,569 already-linked,
task-owned Rust codegen objects reclaimed about **5.9 GiB**, while preserving
libraries, metadata, executables, source and logs. The
[cleanup record](cpu-extensions-20261125/cpu-extension-cache-cleanup.json) binds
the before/after free-byte measurements. Host space was about 9.4 GiB after the
final local checks; the existing VM still had about 1.4 GiB free. Evidence export
skips rootfs and duplicate runner binaries, and per-case scratch roots are removed.
