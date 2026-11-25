# 4 KiB memory and paired performance acceptance

The original application and library instructions are the inputs to both native
Linux and Trames. The acceptance controller is external to the product. No guest
instruction patch, runtime source scan, per-instruction counter, coordinator or
resource service thread is part of this work.

## Core memory gate

`tests/acceptance/validate-memory.py` builds one static original ELF with every
LOAD segment aligned to 4096 bytes and checks the ELF program headers. It requires
an actual Linux 4096-byte host page size. Its deliberately 4 KiB-only addresses
have an offset of 4096 within both 16 KiB and 64 KiB boundaries.

The 11 native references cover geometry/zero-fill, independent protection,
unmap/replacement, adjacent file owners/aliases, file offsets/private COW,
EOF/truncate/grow/SIGBUS, discard/remap, fork COW, concurrent permission faults,
bounded retirement and exec/CLOEXEC. The same original ELF runs through SMCCC
and MMIO with one, two and four workers: **77 cells** including native references.
Signals check the original fault address and code. Expected faults use
sigsetjmp/siglongjmp, so this also catches artificial signal-depth limits.

```sh
python3 tests/acceptance/validate-memory.py --native-only --output /new/native
python3 tests/acceptance/validate-memory.py \
  --runner /frozen/trames --receipt /frozen/build.json --output /new/kvm
```

Hosted Linux CI checks the native reference. The labelled KVM workflow requires
all 77 cells; a missing KVM or an unsupported host page size fails the gate.
The Vulkan/KVM buffer test additionally places code and data at 4 KiB-only
addresses with independent read-only sentinel pages around the imported range.
Its software-driver result does not qualify physical GPU operation.

## Original backing ownership

The VFS retains the physical file descriptor, local window offset and original
byte ownership when publishing a mapped page. KVM uses that explicit capability
instead of trying to infer ownership from an arbitrary pointer. SharedFile
logical truncate zeros bytes/updates EOF without shrinking retained physical
storage. Native FD consumers retain their no-physical-shrink obligation; the
new constructor states that requirement as an unsafe Rust contract.

Foreign files and composite sparse windows do not acquire this capability.
Distant single windows retain their actual inode and local file offset. Opaque
FD exports still require the existing grow/shrink seals. The original page guard
domain and device synchronization responsibility are unchanged. Admission does
not add an fstat, FD duplication, lock or barrier to each guest byte access.

Pending architectural maintenance belongs to the EL1 mailbox after publication.
An interruption before its acknowledgement must carry those flags to the next
restore. The stopped-root checkpoint now preserves them, including permission
revocations. Normal completed syscall entry still uses the existing path.

## Paired performance gate

```sh
python3 tests/acceptance/benchmark.py \
  --runner /frozen/trames --receipt /frozen/build.json --output /new/baseline
python3 tests/acceptance/benchmark.py --audit --output /new/baseline
```

The default schedule covers 13 workloads at one, two and four workers, with
native Linux plus both KVM transports. Each configuration has a native
correctness calibration, a separate KVM warmup and five measured samples per
lane. The seeded schedule interleaves native and KVM samples. It retains
**585 measured samples and 117 calibration/warmup executions**. Native and KVM
have the same CPU affinity per worker count. Pipe/socket/futex cases use two
communicating application threads even at one execution CPU.

Workloads cover dependent integer computation, prefaulted 8 MiB arrays per
thread, raw getpid, cached 4 KiB file I/O, pipe/socket roundtrips, futex handoff,
mmap/protection/unmap, fork COW, fork/exec, timed idle, original make/GCC builds
and original Node worker threads. Native and KVM run the same work, binaries,
libraries and Node scripts. Compiler objects and workload checksums must match
the native calibration before timings are accepted. Mutable outputs are cleaned
between cases; immutable input hashes are checked after the complete run.

Each result retains the command, controlled environment, stdout/stderr hashes,
actual workload/iterations/checksum and raw timing/resource data. Source/build
receipts bind the runner. Installed tools, dependencies, compiler versions,
fixture sources, controller, actual host/kernel/page size and affinity are bound
in inputs.json. An independent audit rejects incomplete schedules, changed logs,
changed sample metadata, failed executions and differing results.

Two clocks answer different questions:

- `work_ns` is the original application's monotonic measurement. Setup and RAM
  prefault occur before that region. Worker-local KVM translations can still be
  admitted on first use inside the region; this is a real execution cost.
- `wall_ns` includes process/session startup, loading, setup and shutdown. An
  external pidfd observer notices exit without subprocess polling quantization.

External wait4 records CPU time, context switches and page faults, including
descendants reaped by their parents. The 250 ms RSS/PSS process-tree snapshots
can miss short peaks; summed RSS counts shared mappings more than once. wait4
maxrss is the largest single process, not a simultaneous sum of all children.
PSS is reported only for complete snapshots. The observer runs on a spare CPU
when available and can still perturb measured workloads through OS sampling.

Host page caches are retained. Separate process warmups do not establish a cold
page-cache measurement or eliminate application JIT startup. Inner workload and
total wall time distributions are reported separately. This is a scoped host
baseline, not a universal near-native speed guarantee. Bare-metal ARM64, combined
KVM/accelerated-GPU hardware and amd64 need their own receipts and measurements.
KVM exits and ioctl counts require a **separate traced diagnostic**, excluded
from the primary timing distribution.

## Product release boundary

Core memory success is not complete product 4 KiB qualification. Release still
requires same-host KVM plus accelerated graphics, image/FD/fence import/export,
device alignment and fallbacks, physical PCM recovery/clocks, A/V synchronization
and hardware video. Missing hardware remains an explicit pending gate.
