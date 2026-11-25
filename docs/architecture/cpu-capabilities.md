# CPU capabilities and task state

The guest contract is the intersection of actual initialized KVM vCPU features
and complete runtime state/Linux ABI support. Host auxv alone cannot establish
what the nested vCPU can execute. The ARM64 adapter reads its ID registers after
`KVM_ARM_PREFERRED_TARGET`/`KVM_ARM_VCPU_INIT`, caches one process profile, and
checks each worker's vCPU against it only when that vCPU is created. Shared
`Cpu::features`/`feature_names` feed ELF auxv and `/proc/cpuinfo`. Doctor reports
the actual ID registers and separate KVM capability availability.

Current passthrough covers instructions using existing GPR, SIMD, NZCV, FPCR and
FPSR state: crypto, CRC, LSE, FP16, RDM, dot product, BF16, AFP and other decoded
stateless extensions when present. A feature available in KVM does not imply
the runtime advertises it. SVE/SME, pointer authentication, MTE, BTI and additional
task controls need their own complete context/signal/ptrace/prctl contract first.
On the existing VM, 32 feature names are advertised; other hosts get their own
profile. This is not a fixed CPU model or an amd64 implementation.

Signals and FPSIMD ptrace regsets retain the complete 32-bit FPCR/FPSR fields,
matching Linux. Hardware handles architectural RES0 behavior. An obsolete mask
must not silently discard AFP FIZ/AH/NEP or EBF16 controls. The ordinary state
save path already includes these registers; no new per-instruction probing,
barriers, shared resource locks, or helper threads are added.

Debugger stepping uses ARM64 architectural single-step, preserving original
application and library bytes. Separate normal/debug EL1 restore entries retain
the ordinary entry path. MDSCR changes only when a task enters/leaves debug mode;
normal syscall exits keep their mailbox resume path. Fault retries preserve the
pending invalidation state. Debug flags remain private to the executor and do
not appear as guest NZCV. Linux arm64 syscall-completion stops report code 0,
followed by a pending TRAP_TRACE before the next instruction if SINGLESTEP is
requested again. The shared kernel delivers that stop on the original worker,
without executing or charging a fictional instruction. CONT clears it.

Exact instruction budgets remain unsupported. SVE/SME extended context is not
implemented in this KVM backend; the current nested VM reports no SVE support.
SVE needs KVM vector-length negotiation/finalization, full Z/P/FFR state and the
matching Linux ABI. No private SME KVM ABI is assumed. See the
[KVM API](https://docs.kernel.org/virt/kvm/api.html),
[Linux arm64 SVE ABI](https://docs.kernel.org/arch/arm64/sve.html), and Linux's
[signal](https://github.com/torvalds/linux/blob/master/arch/arm64/kernel/signal.c)
and [ptrace](https://github.com/torvalds/linux/blob/master/arch/arm64/kernel/ptrace.c)
implementations.

`scripts/validate-cpu.py` compiles an original ELF, binds its source/binary hashes
and a frozen runner receipt, then compares native results with both transports
at one/four workers. It checks advertised instructions and exact CPU flags,
nonzero extended floating-point state across 256 signal frames/four threads,
and original-instruction plus syscall ptrace stops and FP regsets. The fixture
refuses untested advertised bits. New feature families on another host therefore
require corresponding instruction fixtures before that host qualifies. Hardware
CI runs this gate explicitly on the self-hosted ARM64/KVM workflow; hosted builds
alone do not establish hardware acceptance.

The [qualification record](../validation/cpu-extensions-20261125.md) distinguishes
current evidence, exact-source reuse, historical failures and pending work. A
[GPU VM candidate assessment](gpu-test-vm.md) is independent of CPU passthrough.
