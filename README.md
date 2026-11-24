# Trames Linux

Linux userspace product with KVM execution and the shared
[Trames runtime](https://github.com/kb5000/trames-runtime).

Original ARM64 EL0 instructions execute unchanged. A private EL1 adapter saves
state into a mailbox and exits to the same host worker. `trames-session` owns
Linux process/syscall semantics, ELF loading, signals, VFS and scheduling.

## Current implementation

- `trames doctor`: real host, KVM and page-size diagnosis.
- `trames probe`: original L0 fixtures, with SMCCC/HVC and MMIO exit validation.
- `trames run`: foreground static ELF and dynamic ELF from an explicit rootfs,
  using the shared runtime `Cpu`/worker-local `Engine` contract.
- Sparse 48-bit user VA with independent 4 KiB PTEs. Private EL1 controls live
  in the upper address space, outside the runtime's user address range.
- Canonical memfd RAM with host/hardware aliases, bounded arena registrations,
  shared hardware page/range exports, COW, unmap/reuse and empty-table retirement.
- Host byte borrows revoke the hardware alias synchronously. A blocked vCPU
  returns its execution slot with the runtime's original `HardwareWait`.
- Baseline GPR/FP/SIMD/TLS state, deadline/kick, partial EL1 save/restore recovery,
  task migration and negotiated native-bridge trap normalization.

The current product is a foreground execution foundation. Graphics/audio/video
providers, persistent environment management, debugger single-step and extended
SVE/SME state are still pending. Raw external `SharedMapping` imports require
an FD/alias contract; the backend refuses unsupported imports. Complete runtime
and media 4 KiB operation remains a release gate, beyond these tested workloads.
An amd64 execution adapter and its shared guest ABI are planned, not implemented.

## Build and run

```sh
git clone --recurse-submodules git@github.com:kb5000/trames-linux.git
cd trames-linux
cargo build --workspace --release --locked
target/release/trames doctor --json
```

On Linux ARM64 with a real 4096-byte host page and `/dev/kvm` group access:

```sh
target/release/trames doctor --json --require-kvm
target/release/trames run --cpus 4 -- ./hello-static arg1
target/release/trames run --rootfs /path/to/rootfs --cpus 4 -- /bin/sh -c 'echo hello'
target/release/trames run --timeout-ms 1000 -- ./bounded-workload
```

Without `--rootfs`, the specified host ELF is imported as `/program` into a fresh
memory VFS. With `--rootfs`, the selected directory is mounted read/write at guest
`/`, and the program is an absolute guest path. The shared VFS/ELF loader resolves
its interpreter and libraries. Guest exit codes are preserved; a host timeout
cancels the session with an error. Timeout wakeups cover both running and sleeping
guests through existing worker and host-wait mechanisms.

`TRAMES_KVM_TRANSPORT=auto|smccc|mmio` selects the session transport when its VM is
created. Auto selects SMCCC when supported, otherwise MMIO; explicit unsupported
choices fail. Normal SMCCC notifications resume through the mailbox with one
RUN and no register resets. MMIO completes its pending operation before handing
state to another task. Asynchronous EL0 state capture remains a cold register
read path and needs further performance work.

macOS and other unsupported hosts can build/run diagnosis and unit checks;
execution fails explicitly. The baseline guest profile advertises only FP/ASIMD,
uses shared syscall clock fallback, and promises no exact instruction budget.
No helper/coordinator thread, polling bus or application code rewrite is added.

## Validation and remaining work

```sh
python3 scripts/check-boundaries.py
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

On real supported KVM hardware:

```sh
cargo test -p trames-kvm --locked -- --ignored --test-threads=1
TRAMES_KVM_TRANSPORT=mmio cargo test -p trames-kvm --locked -- --ignored --test-threads=1
bash scripts/validate-hardware.sh
python3 scripts/validate-session.py
TRAMES_KVM_TRANSPORT=mmio TRAMES_VALIDATION_OUTPUT=target/validation/session-mmio python3 scripts/validate-session.py
```

The session script requires a native C compiler and static libc; it compares
native and KVM execution, including real dynamic-loader rootfs fixtures. The
normal CI is build/unit validation. A separate labelled-runner workflow enforces
hardware tests; missing hardware fails rather than silently skipping acceptance.

[Implementation ledger](docs/implementation.md)
· [Architecture](docs/architecture.md)
· [L0 record](docs/validation/l0-20261124.md)
· [Shared-session record](docs/validation/session-20261124.md)
· [Accepted product plan](https://github.com/kb5000/trames-ipad/blob/e93f62b9/docs/roadmap/trames-linux-kvm.md)
