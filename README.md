# Trames Linux

Linux userspace product with KVM CPU execution and the shared
[Trames runtime](https://github.com/kb5000/trames-runtime).

Development has started with an ARM64 hardware bring-up batch. Original EL0
instructions execute unchanged; a small private EL1 adapter saves state into a
mailbox and exits to its owning host worker. Linux process/syscall semantics
will use `trames-session`; this repository does not boot a guest Linux kernel.

## Current implementation

- `trames doctor`: actual host, KVM and page-size capability diagnosis.
- `trames probe`: real KVM execution of separately loaded original fixtures,
  using SMCCC/HVC or MMIO exits and the shared runtime's `Aarch64State` layout.
- Independent 4 KiB stage-1 mappings, canonical host/guest RAM, user read-only
  and unmapped-page faults, PTE/TLBI repair, and private EL1 mailbox protection.
- GPR, NZCV, SP, TLS and FP/SIMD round trips; a syscall-free loop interrupted
  by a kernel thread-targeted one-shot timer. No helper/service thread or polling.
- Explicit runtime submodule pin; the active iPad/Mac runtime worktree is untouched.

This is an L0 transport prototype. General ELF/rootfs execution, the shared
`Cpu`/session adapter, concurrent backing borrows/COW, signals/task migration,
and full product graphics/audio/video are not implemented yet. A successful
probe proves only the checks listed in its result, not product compatibility.

## Build and run

```sh
git clone --recurse-submodules git@github.com:kb5000/trames-linux.git
cd trames-linux
cargo build --workspace --release --locked
target/release/trames doctor --json
```

On Linux ARM64 with a real 4096-byte host page and `/dev/kvm` access:

```sh
target/release/trames doctor --json --require-kvm
target/release/trames probe --transport smccc --iterations 10000 --json
target/release/trames probe --transport mmio --iterations 10000 --json
bash scripts/validate-hardware.sh
```

Use the host's `kvm` group for ordinary-user device access. `--transport auto`
selects SMCCC when available and otherwise MMIO; it expresses capability
selection, not a measured performance ranking. An explicitly requested
unsupported transport fails. macOS and other unsupported execution hosts can
build/run diagnosis and common unit tests; `probe` fails instead of emulating
or claiming a hardware pass. An amd64 execution adapter is planned, not present.

Each probe uses one 64 KiB RAM arena/one KVM slot and one thread-affine vCPU.
Its loop timing excludes 16 warm-up exits but includes full-state checks and
mailbox replies. It is not an isolated VM-exit measurement. Record whether the
host is bare metal or a nested VM; nested measurements cannot predict bare-metal
overhead. The probe's syscall handler only checks transport state and returns
fixture values; it does not implement an alternative Linux kernel.

## Validation and next batches

```sh
python3 scripts/check-boundaries.py
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The normal CI jobs are explicitly build/unit checks. The separate hardware
workflow requires an ARM64 runner labelled `trames-kvm` and real `/dev/kvm`;
absence of hardware fails that workflow. Linux timers are tested with blocking
sleep interrupted by a real kernel signal, without requiring KVM.

The next dependency batch connects canonical `HardwareStorage`/`HardwareRange`
and the shared `Cpu`/session interface, including revoke/borrow/COW, preemption
state capture, signals and task handoff. Graphical/media providers then share
one image/FD/synchronization contract: native Vulkan, Wayland/X11 presentation,
VA-API/V4L2 video, and PipeWire/ALSA PCM. Complete 4 KiB operation remains a
release requirement throughout these paths.

[Accepted architecture and delivery plan](https://github.com/kb5000/trames-ipad/blob/e93f62b9/docs/roadmap/trames-linux-kvm.md)
· [L0 architecture](docs/architecture.md)
· [Implementation ledger](docs/implementation.md)
· [Validation record](docs/validation/l0-20261124.md)
