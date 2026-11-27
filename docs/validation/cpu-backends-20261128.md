# Interpreter / JIT / KVM CPU comparison — 2026-11-28

Result: in the matched production-worker CPU microbenchmarks, **JIT and KVM are
in the same range on the integer accumulator loop**. KVM is **21.6–36.3x faster
than JIT on the measured scalar-memory loops**. JIT is **1.21–1.29x faster than
the interpreter on those worker memory paths**. The nested-KVM SVC boundary is
much slower than either software boundary. These are different workload regimes,
not one overall application-speed ranking or a bare-metal performance ceiling.

## Configuration and equal-work contract

- Same existing Mac-hosted Lima/VZ ARM64 Linux VM, six vCPUs / 8 GiB, actual 4096
  byte pages, Linux `7.0.0-44-generic`. All processes use guest vCPU affinity 2.
  **KVM is nested inside this VM**, not bare-metal Linux KVM.
- One release executable contains all three statically dispatched engines;
  same nightly-2026-10-16 toolchain, thin LTO, overflow checks, incremental off.
- Linux KVM transport source `9db947eba448b871f31ca06511c46430bfb35387` and
  one common Runtime `89c5abd57ce5cc8341f17fcea35a339b0f991595`. The latter is
  supplied only to an isolated benchmark checkout. The Linux product's actual
  Runtime pin `8bd058556ffb35be8e127afbaf7c1c26dae998d9` remains unchanged.
- All three use `Cpu::run_parallel` with one retained canonical Memory/View.
  Backend-specific hardware admission is outside timing; KVM VM/vCPU creation,
  initial mapping faults and JIT translation are covered by the excluded warm-up.
  Ordinary instructions retain original guest bytes. Explicit SMCCC transport is
  required; device access, KVM API, 4 KiB pages and forwarding are checked, and
  actual KVM guest execution/checksums must succeed.
- Compute uses finite counter loops and one final original SVC. KVM has no exact
  instruction count; counter/x0/PC and final canonical data prove matched work.
  Software-reported counts must equal the known program count. The common exact
  software budget is 1,048,576 instructions; the KVM product's 2 ms wall-time
  assignment and short-SVC budget retention remain enabled. Normal yields are
  included and recorded, not disabled to favor a lane.
- Short SVC loops include the same ordinary backbranch on every iteration and
  a simulated x0 return. **No shared Linux syscall handler, process scheduler,
  JNI, Android lifecycle, I/O, graphics or application startup is measured.**

Each process runs one warm-up plus five timed samples per workload. Six process
orders use all backend permutations, putting each backend in every position twice.
The reported value is the median of six process medians; all 540 timed samples
and 108 warm-ups are retained. Host byte validation occurs after all samples,
so it does not revoke KVM data aliases between timed passes. No new VM, SDK,
NDK/JDK, rootfs image or emulator was installed.

## Matched observations

All elapsed times below are **milliseconds**, lower is faster. SVC rows run
10,000 exits; compute/memory rows execute the full stated finite loop.

| Original guest workload | JIT | Interpreter | KVM |
| --- | ---: | ---: | ---: |
| Integer, 262,144 iterations × 31 ADD + SUBS/B.NE | 1.833 | 8.284 | 1.907 |
| Scalar memory, 65,536 × LDR/add/STR/SUBS/B.NE | 1.866 | 2.401 | 0.0863 |
| Scalar memory, 1,048,576 iterations | 30.811 | 37.315 | 0.8496 |
| 10,000 SVC + backbranch + simulated returns | 0.0764 | 0.1741 | 352.031 |
| 10,000 ADD + SVC + backbranch + returns | 0.0764 | 0.1800 | 351.923 |
| 10,000 × 16 ADD + SVC + backbranch + returns | 0.0786 | 0.2637 | 351.077 |

The integer process medians overlap: JIT 1.776–1.916 ms, KVM 1.873–1.924 ms.
The 3.9% central difference does not establish JIT superiority over native hardware.
The interpreter takes about 4.34x KVM's time here. This is one simple accumulator
payload with very little memory traffic, not compiler/build/app performance.

For memory, JIT takes 21.63x / 36.26x KVM's time; the interpreter takes 27.82x /
43.92x. JIT improves on the interpreter by 1.29x / 1.21x. This supports prioritizing
ordinary-memory helper/translation overhead: native ALU coverage alone cannot
close the observed memory gap. No per-function cycle attribution is inferred.

SVC-only is 7.64 ns / 17.41 ns / **35.20 microseconds** per return for JIT /
interpreter / KVM. That includes the guest backbranch, backend CPU exit, simulated
return and ordinary KVM assignment renewals. KVM reports 172–182 yields per
10,000 returns, versus zero software yields. The large nested-virtualization
transport cost must not be presented as bare-metal KVM latency, or as a whole
Linux syscall being thousands of times faster in JIT. Real syscall work and
bare-metal measurements remain separate.

Compute software yields are eight per integer pass and five per large memory
pass; KVM completes those payloads before its wall deadline, with zero yields.
Both software/hardware progress policies are unmodified. All final counters,
canonical data and PCs match; all original code bytes remain unchanged and all
JIT payloads finish with zero interpreter fallback.

## Why the previous interpreter memory number differs

The preceding [Runtime fusion report](https://github.com/project-trames/trames-runtime/blob/5ca460c401745c4a631bbeacbb829e687598231f/docs/validation/jit-memory-fusion-20261127.md)
used the direct `Memory` API: about 0.931 ms for the interpreter and 1.854 ms for
JIT on 65,536 iterations. **Those numbers must not be mixed into this worker table.**
Here the interpreter uses the production parallel View and takes 2.401 ms; JIT
is 1.866 ms. Direct Memory and worker View have different existing access paths
and ownership costs. The earlier direct-path result remains valid within its
scope, but it does not imply that JIT loses to the interpreter on every memory
interface. No aggregate speedup is derived by mixing those scopes.

## Verification and retained evidence

The isolated package passes release compilation, strict all-target Clippy and
formatting on actual ARM64 Linux. Quick hardware qualification passes for all
three engines, followed by six balanced formal groups. Counters, work and PCs
are checked on every pass; original code and canonical host bytes are checked
after all timings. Hardware counts remain explicitly unavailable rather than
invented from the payload.

An [offline audit](cpu-backends-20261128/audit-result.json) rechecks all raw records,
process order, known/software counts, cross-backend work/results, six-process
statistics and current measurement/KVM source bindings. Five negative cases reject
missing processes, mismatched instructions, changed data, false medians and false
KVM exact-count claims. It performs no hardware rerun.

[All raw samples](cpu-backends-20261128/),
[full medians/yields](cpu-backends-20261128/summary.json),
[compressed source/binary/build receipt](cpu-backends-20261128/receipt.json.gz),
[benchmark source and reproduction](../../tools/cpu-backends/README.md) and
[balanced measurement controller](../../scripts/benchmark-cpu-backends.py) are
retained. Empty process stderr files and temporary source/build products are not
archived. Production code/dependencies/pins are unchanged, so unrelated product,
Linux application, network and graphics acceptance is not rerun for this report.
