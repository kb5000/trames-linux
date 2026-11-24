# Native resource and KVM import validation — 2026-11-25

Shared runtime: `47f34e9e9632c2c650f8fc0c016b7576775e77dc`.
The implementation uses original guest instructions and original native allocation
bytes. No coordinator/service thread or guest-instruction patching was added.

## Host and scope

The existing Lima/VZ debug VM ran ARM64 Linux 7.0.0-43-generic with real 4096-byte
host pages and nested KVM. The existing lavapipe ICD supplied Vulkan 1.4.318 and
EXT_external_memory_host. There is no GPU render node, desktop, PCM or video
device in this VM. Vulkan command execution here establishes software-driver
API/memory interop only. It does not establish hardware GPU/media performance.

## Results

- Strict product workspace Clippy/all-targets passed on ARM64 Linux and macOS.
  Runtime memory/graphics/Vulkan/Wayland strict all-target checks passed on both;
  the Mac check also included VFS. Linux release and Mac unit builds passed.
- Linux runtime: memory 243 unit + 6 integration tests, graphics 3, Vulkan 66
  and Wayland 27 passed. Existing ignored tests remain ignored. Mac memory:
  242 unit + 6 integration passed; CoreVideo/Metal graphics 3 and Wayland 28
  passed. CoreVideo cases require the normal native host environment.
- Mac product build check and iOS ARM64 FFI check passed. The FFI frame-record
  ABI/viewport unit test passed after checked Metal-handle extraction.
- Each of SMCCC/MMIO passed 10 KVM hardware checks and one Vulkan interop test:
  [SMCCC](resources-20261125/smccc.log), [MMIO](resources-20261125/mmio.log).
- Each transport passed all 21 native/KVM foreground session records, including
  one/four workers, static/dynamic ELF, fork/COW, page faults/protection, TLS/futex
  threads and running/sleeping deadlines:
  [SMCCC](resources-20261125/smccc-sessions.json),
  [MMIO](resources-20261125/mmio-sessions.json).

The new hardware cases retain image bytes after native image release and fork;
duplicate an FD into a different canonical VA; prove both imports have one GPA;
keep a 4 KiB neighbor live while an aliased page is borrowed; and verify that
ending one borrow does not prematurely restore hardware access held by another.

The Vulkan case runs native fill, KVM load/add/store/SVC, then native buffer copy
into a second allocation. Both directions observe the original words. Coherent
upload/download scan counters remain unchanged. Closing Vulkan device/instance
handles leaves the imported guest bytes live. Generic driver allocations and
unproven opaque handles remain unsupported for KVM import.

Session comparisons ran the same final executable source before the embedded
runtime pin metadata advanced from the previous pin. Only that metadata was
then rebuilt; the final [doctor record](resources-20261125/doctor.json) and
[binary digest](resources-20261125/binary.sha256) identify the matching release.
Unaffected functional passes were reused. A disk-full test build was resolved by
removing this task's disposable incremental cache, then rerunning failed checks.

## Post-push verification

The trames-linux [three-platform CI](https://github.com/kb5000/trames-linux/actions/runs/45444247216)
passed on Linux amd64, Linux arm64 and macOS for commit
`cbd5d5c2cd54289c468ead75e4d79de60587e765`.
The runtime [Linux full CI](https://github.com/kb5000/trames-runtime/actions/runs/45443561922)
passed. Local macOS `cargo test --workspace --all-features --locked` passed for
that exact runtime commit, and Mac product all-target/all-feature strict Clippy
passed. Remaining hosted Mac/iOS jobs were still queued due to runner capacity
when these local results were recorded; no cloud pass is inferred for them.

## Reproduction and remaining gates

Use the pinned toolchain. On an ARM64/4 KiB KVM host, run product strict checks,
then `cargo test -p trames-kvm --lib --test hardware -- --ignored --test-threads=1`
with each TRAMES_KVM_TRANSPORT. Opt into `--test vulkan` when an ICD supports
external host memory. `scripts/validate-session.py` accepts TRAMES_BINARY for
builds using a separate Cargo target directory. The hardware workflow keeps
Vulkan opt-in so a CPU-only KVM host remains usable.

Linux presentation, GPU image/DMA-BUF providers, PCM playback/capture and hardware
video decode/encode remain unimplemented. Real hardware, application compatibility,
resource pressure and bare-metal performance acceptance remain release gates.
