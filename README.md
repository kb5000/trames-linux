<p align="center">
  <img src="https://raw.githubusercontent.com/project-trames/.github/main/assets/trames-icon.png" alt="Trames icon" width="112">
</p>

<h1 align="center">Trames Linux</h1>

<p align="center">
  <strong>A high-performance KVM execution engine powered by a shared userspace Linux kernel.</strong>
</p>

<p align="center">
  Run unmodified ARM64 Linux binaries with hardware-accelerated CPU execution and isolated userspace OS services.<br>
  KVM accelerates hardware execution and page translation; Trames Runtime handles all system calls without booting a guest kernel.
</p>

<p align="center">
  <a href="https://github.com/project-trames/trames-linux/actions/workflows/build.yml"><img src="https://github.com/project-trames/trames-linux/actions/workflows/build.yml/badge.svg?branch=main" alt="Build and unit checks"></a>
  <a href=".github/workflows/hardware.yml"><img src="https://img.shields.io/badge/execution-KVM-7c5269?style=flat-square" alt="KVM execution"></a>
  <a href="#requirements"><img src="https://img.shields.io/badge/host-Linux%20ARM64-30303d?style=flat-square&amp;logo=linux&amp;logoColor=white" alt="Linux ARM64 host"></a>
  <a href="#requirements"><img src="https://img.shields.io/badge/pages-4%20KiB-f0ad43?style=flat-square" alt="4 KiB host pages"></a>
  <a href="rust-toolchain.toml"><img src="https://img.shields.io/badge/Rust-nightly-b65437?style=flat-square&amp;logo=rust&amp;logoColor=white" alt="Rust nightly"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-7c5269?style=flat-square" alt="GPL-3.0-or-later"></a>
</p>

<p align="center">
  <a href="#features">Features</a> ·
  <a href="#architecture">Architecture</a> ·
  <a href="#quickstart">Quickstart</a> ·
  <a href="#graphics-and-audio">Graphics &amp; Audio</a> ·
  <a href="#building-and-testing">Building</a> ·
  <a href="#documentation">Documentation</a>
</p>

---

## Features

Trames Linux pairs Linux's native Kernel-based Virtual Machine (KVM) with the userspace runtime engine shared across Mac and iPad:

* **Near-Native KVM Acceleration**: ARM64 user instructions execute directly on hardware vCPUs. A lightweight trap handler routes syscalls and page faults to the userspace runtime with minimal overhead.
* **Zero Guest Kernel Bloat**: Boots instantly without a guest vmlinux image. Syscalls, signal dispatches, futexes, and process lifecycles are serviced directly by `trames-runtime`.
* **Flexible Execution Contexts**: Run raw static ELF executables, mount full distribution rootfs directories, or launch interactive terminal shells with complete PTY job control.
* **Virtual Memory & Storage**: Sparse 4 KiB guest memory mapping with copy-on-write, direct directory passthrough, and persistent state stores.
* **Configurable Networking**: Flexible socket policies supporting isolated loopback, outbound-only filtering, or full host bridge access.
* **Hardware Compute & Display**: Headless Vulkan compute acceleration, X11 fixed-surface window presentation, and low-latency ALSA PCM audio playback/capture.
* **Built-in Diagnostics**: Comprehensive host environment validation via `trames doctor` and transport stress testing via `trames probe`.

---

## Architecture

Trames Linux separates physical instruction virtualization from operating system abstractions:

```text
┌─────────────────────────────────────────────────────────────┐
│                 Linux ARM64 Host System                     │
│           Host OS Kernel · /dev/kvm · X11 / ALSA            │
└──────────────────────────────┬──────────────────────────────┘
                               │
┌──────────────────────────────┴──────────────────────────────┐
│                    trames-linux-app                         │
│           CLI Host · Session Management · Diagnostics       │
│   ┌─────────────────────────────────────────────────────┐   │
│   │             trames-runtime (Userspace Kernel)       │   │
│   │       VFS · Tasks · Sockets · Soft Page Tables      │   │
│   └──────────────────────────┬──────────────────────────┘   │
│                              │ Syscall Dispatch / Page Faults
│   ┌──────────────────────────┴──────────────────────────┐   │
│   │             trames-kvm (Execution Engine)           │   │
│   │        vCPU Loop · SMCCC / MMIO Mailbox Trap        │   │
│   └──────────────────────────┬──────────────────────────┘   │
└──────────────────────────────┼──────────────────────────────┘
                               │ Hardware vCPU Execution (ARM64)
┌──────────────────────────────┴──────────────────────────────┐
│                  Unmodified Linux Binaries                  │
└─────────────────────────────────────────────────────────────┘
```

* **Syscall Trap Mechanism**: When guest code invokes a syscall or encounters a fault, execution traps into `trames-kvm` via SMCCC (`HVC`) or MMIO notifications, returning through the host KVM interface to `trames-runtime` for userspace handling.
* **Transport Selection**: The runtime supports both `smccc` and `mmio` mailbox protocols, selected automatically or configured explicitly via `TRAMES_KVM_TRANSPORT`.

---

## Requirements

* **CPU Architecture**: ARM64 (AArch64) host.
* **Page Size**: 4096-byte (4 KiB) host page configuration.
* **Virtualization**: Functional `/dev/kvm` with read/write access permissions.

Verify your host environment using the built-in diagnostic tool:
```sh
target/release/trames doctor --json --require-kvm
```

---

## Quickstart

### Build the Binary

```sh
# Clone repository with required submodules
git clone --recurse-submodules https://github.com/project-trames/trames-linux.git
cd trames-linux

# Build the guest interop client and release CLI
python3 runtime/scripts/build-guest-cli.py
cargo build --workspace --release --locked
```

### Running Binaries & Shells

Run a standalone static ARM64 binary:
```sh
target/release/trames run --cpus 4 -- ./hello-static arg1
```

Run dynamic applications inside an explicit rootfs directory:
```sh
target/release/trames run --rootfs /path/to/rootfs --cpus 4 -- /bin/sh -c 'echo hello'
```

Spawn an interactive terminal session inside the guest rootfs:
```sh
target/release/trames shell --rootfs /path/to/rootfs
```

Execute commands with explicit environment variables, working directory, and timeout:
```sh
target/release/trames exec --rootfs /path/to/rootfs \
  --cwd /tmp \
  --setenv MODE=production \
  --timeout-ms 5000 \
  -- /usr/bin/env
```

### Persistent Storage Modes

* **Direct Rootfs Mount (`--rootfs`)**: Directly mounts a host directory as the guest root (`/`). Modifications made by the guest write directly to the host directory.
* **Managed Store (`--store`)**: Initializes an isolated copy in a dedicated persistent store directory:

```sh
# Initialize a persistent store from a base rootfs template
target/release/trames run --store /path/to/store --rootfs /path/to/rootfs -- /bin/sh -c 'echo "state" > /root/data'

# Reopen the existing store directly
target/release/trames run --store /path/to/store -- /bin/cat /root/data

# Extract files from a stopped store without spawning a vCPU
target/release/trames read-file --store /path/to/store -- /root/data
```

---

## Graphics and Audio

### Vulkan Compute & Presentation

Trames Linux supports headless Vulkan compute and windowed presentation via X11 when a compatible host Vulkan library and matching guest ICD are configured. Use `--vulkan-icd` to install the guest SDK; `--vulkan` uses an already installed SDK:

```sh
# Windowed Vulkan presentation via X11
target/release/trames run --rootfs /path/to/rootfs \
  --vulkan-icd /path/to/libtrames_vulkan_icd.so \
  --display x11 --display-size 1024x768 \
  --setenv VK_DRIVER_FILES=/usr/share/vulkan/icd.d/trames.json \
  -- /path/to/guest-vulkan-app

# Headless Vulkan compute (omit --display)
target/release/trames run --rootfs /path/to/rootfs \
  --vulkan \
  -- /path/to/vulkan-compute-benchmark
```

### ALSA Audio

Audio playback and input capture can be toggled independently:

```sh
# Audio playback routed to host ALSA/PipeWire/PulseAudio
TRAMES_AUDIO_OUTPUT_DEVICE=default target/release/trames run \
  --audio \
  --rootfs /path/to/rootfs \
  -- /usr/bin/aplay /path/to/audio.wav

# Audio input recording
target/release/trames run --audio-input --rootfs /path/to/rootfs -- /usr/bin/arecord /tmp/test.wav
```

---

## Building and Testing

### Prerequisites

* Rust nightly toolchain (specified in [`rust-toolchain.toml`](rust-toolchain.toml)).
* Python 3 and a native C compiler (`gcc` or `clang`).
* `libasound2-dev` (Debian/Ubuntu) for native ALSA ABI bindings.

### Workspace Checks

```sh
python3 scripts/check-boundaries.py
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

### Hardware & KVM Tests

Run hardware-level integration tests on an ARM64 host with KVM enabled:

```sh
# Test default SMCCC transport
cargo test -p trames-kvm --lib --test hardware --locked -- --ignored --test-threads=1

# Test MMIO mailbox transport fallback
TRAMES_KVM_TRANSPORT=mmio cargo test -p trames-kvm --lib --test hardware --locked -- --ignored --test-threads=1

# Full hardware validation script
bash scripts/validate-hardware.sh
```

---

## Documentation

| Domain | Deep Dives |
| :--- | :--- |
| **Architecture & Transport** | [KVM Transport & Memory Ownership](docs/architecture.md) · [CPU Capabilities & States](docs/architecture/cpu-capabilities.md) |
| **Host CLI & Runtime** | [Shared Host CLI Design](docs/architecture/host-cli.md) · [Implementation Ledger](docs/implementation.md) |
| **Acceleration & Media** | [Vulkan Presentation & Compute](docs/architecture/presentation.md) · [ALSA PCM Provider](docs/architecture/audio.md) |
| **Testing & Methodology** | [Acceptance Test Design](docs/architecture/acceptance.md) · [Memory & Performance Acceptance](docs/architecture/memory-performance-acceptance.md) |
| **Evidence & Benchmarks** | [Hardware Validation Records](docs/validation/) · [KVM Memory Publication](docs/validation/kvm-joined-writes-20261126.md) |

---

## Community and Contributing

Bug reports, reproduction cases, benchmarks, and pull requests are welcome. Please read our [Contribution Guidelines](https://github.com/project-trames/.github/blob/main/CONTRIBUTING.md) and [`AGENTS.md`](AGENTS.md) before submitting patches.

* **Issues**: [GitHub Issue Tracker](https://github.com/project-trames/trames-linux/issues)
* **Security**: For private vulnerability reports, see [SECURITY.md](https://github.com/project-trames/.github/blob/main/SECURITY.md)
* **Conduct**: We adhere to the [Project Code of Conduct](https://github.com/project-trames/.github/blob/main/CODE_OF_CONDUCT.md)

---

## License

Trames Linux is licensed under the **GNU General Public License v3.0 or later** ([GPL-3.0-or-later](LICENSE)).<br>
Third-party notices and provenance tracking are maintained across component roots.

---

<p align="center">
  Part of <a href="https://github.com/project-trames">Project Trames</a> · A portable userspace Linux runtime with pluggable execution engines.
</p>
