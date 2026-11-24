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
