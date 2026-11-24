# L0 KVM transport

The product keeps CPU transport separate from shared Linux semantics. The
`trames-kvm` crate depends on the common `trames-execution` state contract.
The product crate is `trames-linux-app`, avoiding the existing runtime syscall
crate named `trames-linux`. Linux-specific fd/ioctl code and AArch64 register/
page-table/assembly code have separate modules. Unsupported guest architectures
are explicit, never silently treated as ARM64.

## Guest geometry

The first real host configuration is Linux/aarch64, `_SC_PAGESIZE == 4096`.
One demand-zero 64 KiB mapping at guest physical `0x40000000` retains all RAM.
Four 4 KiB table pages form a sparse 48-bit stage-1 translation; guest virtual
addresses do not become guest physical addresses. The probe uses 32-bit physical
addresses within the supported default KVM IPA range.

| Guest VA | Purpose | Permissions |
| --- | --- | --- |
| `0x8000` | Product EL1 vectors/resume/save | EL1 read/execute; EL0 inaccessible |
| `0xa000` | Per-vCPU mailbox/context | EL1 read/write; EL0 inaccessible |
| `0xd000` | MMIO doorbell at GPA `0x50000000` | EL1 device write; outside RAM slots |
| `0x10000` | Original probe instructions | EL0 read/execute; EL1 non-executable |
| `0x20000` | Canonical data | EL0 read/write, non-executable |
| `0x21000` | Adjacent independent page | Initially read-only, then repaired |
| `0x22000` | Unmapped hole | No mapping |
| `0x30000` | User stack page | EL0 read/write, non-executable |

PTE permission repair publishes descriptors and asks the EL1 resume prelude to
perform the required TLBI/barriers. Code publication cleans host cache lines
and invalidates the guest instruction cache at fixture creation; it does not
add data-access barriers. No application instruction is rewritten.

## Syscall and fault handoff

```text
original EL0 svc/fault
  -> product EL1 vector: save complete baseline Aarch64State + ESR/FAR/event
  -> HVC #0 using explicitly filtered SMCCC ID 0x86005452, or a 4-byte MMIO write
  -> KVM_RUN returns on the same host worker
  -> read mailbox; validate/handle the L0 fixture; publish result
  -> KVM_RUN completes pending exit work, EL1 restores state, ERET
```

All real arguments remain in the mailbox. SMCCC only routes the notification;
there is no per-register `GET_ONE_REG` in the hot loop. Register ioctl setup
is cold. KVM vCPU, VM and RAM owners remain live through each exit, and close
in that order. Closing a final fixture with a pending exit never reassigns the
vCPU or saves it as a resumable task. Production handoff/parking must complete
all pending KVM operations before reuse; that shared-session work remains open.

The probe handler is deliberately fixture-only. It checks the guest's original
getpid-number SVC entry and returns a sequence value to prove state transfer.
Actual Linux getpid and other syscall semantics belong to shared runtime code.

## Deadline and host memory conditions

A Linux `SIGEV_THREAD_ID` monotonic one-shot timer targets the current worker.
The reserved realtime signal sets its retained run mapping's `immediate_exit`
byte. The kernel signal kicks active KVM execution; the byte also closes the
signal-before-entry window. The guard blocks/drains its signal and clears TLS
before the vCPU mapping can be destroyed. Timer tokens reject late delivery to
a later deadline. There is no `SIGEV_THREAD` or new service/coordinator thread.

The active signal path uses raw pointers/volatile access, not a Rust reference
to `kvm_run`. A thin raw RUN wrapper preserves that condition while the
`kvm-ioctls` crate owns fds/mappings and handles setup. Only the current vCPU
thread invokes RUN. No Rust reference into guest RAM crosses RUN; host RAM
reads/writes happen after execution stops. This is the L0 single-owner case,
not a substitute for runtime's multi-actor borrow/revoke protocol.

The selected baseline excludes SVE/SME, pointer authentication and exact
instruction counts. Full asynchronous preemption-state capture, interruption
inside partial EL1 save/restore, external kick handles and task migration must
be completed in the shared-engine integration batch. L0 only asserts that a
syscall-free loop can be interrupted and its execution slot returned.

## Shared-session engine (second batch)

`kernel::Cpu` owns movable `Aarch64State` and negotiated API flags. `Engine`
lazily owns one creator-thread vCPU and its sparse stage-1 tables. All workers
in this host process retain the same KVM VM, so exported physical storage keeps
one GPA identity across tasks, mm objects and workers. VM/CPU IDs are retired
before reuse; original runtime scheduling and resource objects remain in charge.

The production adapter uses TTBR0 for 48-bit guest user addresses and TTBR1 for
private EL1 controls at the top of the upper address space. User ELF mappings
never collide with low adapter fixture addresses. Both translation granules are
4 KiB; guest VA is independent of the bounded physical arena addresses.

The unprotected ARM64 KVM profile also provides per-vCPU TLB/I-cache context
isolation when a different vCPU of the same VM is loaded on a physical CPU
(`vcpu_prepare_mmu` in the linked ARM64 kernel source). The backend relies on
that existing boundary for its private roots, and issues guest TLBI when it
changes a stopped worker's mappings. It does not add an ordinary-entry flush.

Each 64 MiB demand-zero memfd arena has one KVM slot and two shared mappings:

```text
runtime SharedPage/HardwareRange -> canonical host RW alias -> same memfd bytes
KVM stage-2 backing              -> hardware alias         -> same memfd bytes
```

The existing page/range borrow gate mprotects only the hardware alias to NONE
before returning a Rust host reference. Linux's MMU invalidation withdraws KVM
access synchronously; the canonical alias stays in place. ARM64's EFAULT return
does not provide a portable fault GPA, so an active-lease queue returns an
original `HardwareWait`. Release restores the alias and fires readiness. A lease
generation closes release-before-wait races; an EFAULT repeated without lease
progress is a host error. There is no polling/resource service thread.

The arena remains mapped while any page, range or PTE owner retains it. First
exports replace the runtime's original bytes once, not with a persistent shadow.
COW/admission stays in `View::export_hardware_page` and the common accounting.
PTE invalidation precedes physical-owner retirement; no stopped private table can
be reused without the required TLBI. Empty sparse subtrees retire on unmap.
Final allocations punch their memfd extent only after every byte/PTE owner drops,
then return it to the bounded pool. Data-only host writes do not flush code;
executable exports retain code-publication metadata at the existing write hook.

Normal SMCCC notifications already complete the ARM64 HVC instruction, and the
ARM64 run path has no userspace hypercall completion callback. The saved mailbox
can be replaced and the branch to full restore reused without register ioctls.
MMIO uses an immediate-exit RUN to complete its pending store before handoff.
See the Linux [SMCCC handler](https://github.com/torvalds/linux/blob/master/arch/arm64/kvm/hypercalls.c)
and [ARM64 RUN path](https://github.com/torvalds/linux/blob/master/arch/arm64/kvm/arm.c).

On asynchronous interruption, a stopped EL0 bank is read through cold register
ioctls. A partial restore still has its complete mailbox. A partial save/vector
finishes bounded private code to return the pending SVC/fault rather than losing
it as a yield. The post-HVC and post-MMIO branches are restore checkpoints too.
Tests cover every restore instruction address, an actual hardware-stepped partial
save, normal-path RUN/reset counts, and real task migration. External kicks retain
only a mutex-protected live pthread identity and a pending request; the handler
updates its own TLS run byte, and retirement makes old handles inert.

The shared runtime normalizes data/instruction faults, Linux signals and the
negotiated HLT bridge registry. Unsupported external-pointer imports and debugger
single-step fail explicitly. The current VM has no GPU/PCM/video device; the
common FD/image/synchronization API and Linux providers remain a separate required
product batch. No raw pointer import is allowed to bypass the hardware borrow gate.
