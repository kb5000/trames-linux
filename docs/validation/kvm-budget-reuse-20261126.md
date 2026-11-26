# Worker-local KVM execution budgets — 2026-11-26

Follow-up: the 18 iPad QEMU failures recorded below were closed as conformance-test
issues by Runtime `2c5cc31` / iPad `229838ae`. Final three-lane coverage is
542 passing cases plus three original ignores, using 538 unchanged passes and
four exact rechecks per lane. See the [CI closure record](https://github.com/project-trames/trames-ipad/blob/b9af900494b80c604ebc2334a229890ffbc994d5/docs/validation/runtime-conformance-20261126.md).
The frozen Linux C8 performance cohort and historical CI snapshots below remain
unchanged.

Final Linux `501a77ebcbec55f04792496aa14675f848c839c4` retains one bounded
execution deadline across short syscalls within a Runtime worker assignment.
It releases that registration before parking, changing tasks or invoking a
native provider. Original application instructions and shared Linux syscall
semantics remain unchanged. The final version separates private timer ticks
from explicit cancellation so a tick can preserve restartable host I/O while
cancellation can interrupt it.

## Implementation and source identity

Runtime `c34ad9e8fc5bbd247a253b1b1e734e5ef921ff91` adds `Cpu::begin_execution`
and `finish_execution` at assignment boundaries. Their defaults delegate the
existing interrupt preparation and do nothing at finish. Static dispatch keeps
Apple/interpreter behavior; no new ABI check, source scan or feature probe runs
per syscall. Direct/quiesced calls retain the original per-call lifetime.

KVM preserves the absolute end across short SVC returns; host syscall dispatch
counts against that wall budget. An already expired budget yields without
executing the next instruction or changing CPU state. Normal page-fault retries
retain the timer. A bounded EL1 checkpoint releases it before installing its own
completion deadline. Worker finish and executor destruction retire the guard
while the vCPU mapping is live. There is no new service/coordinator thread,
polling bus, ordinary-memory barrier, page scan or loaded-instruction patch.

The initial version `5e57bb19` used SA_RESTART for its shared timer/kick signal.
A closeout regression exposed a real bug: an explicit kick incorrectly restarted
a blocking host pipe read. That production source is retained with the new test
and its expected failure (read returned one byte instead of EINTR). Final `501a77e`
uses **RTMIN+4 with SA_RESTART for timer ticks**, and **RTMIN+5 without SA_RESTART
for explicit kicks**. Both are masked together while publishing/retiring the
thread-local active target and restoring the original mask. The positive tests
require private-tick restart, explicit-kick EINTR and restoration of both masks.
The first candidate is superseded; its old passing data audit does not qualify
this later-discovered cancellation requirement.

The final controlled comparison uses these immutable runners:

| Role | Linux source | Runtime | Binary SHA-256 |
| --- | --- | --- | --- |
| Before | `f52564f980620807d2baf76f25e6b218ce52d746` | `c34ad9e8` | `ddbcc28a4cf7041ce8137f0542552273e926185c3fb85d062947c492eec46516` |
| Final | `501a77ebcbec55f04792496aa14675f848c839c4` | `c8ac3f9f` | `7cdd6232f047aa9b04ba14ba5ddcddabfc3ff1b7b5432af82792c52e0761ba20` |

Both use shared CLI `8dd30e46dffdaf4fd9c412591edb557748e8d737`, the same toolchain
and build flags, and clean receipts. Runtime C8 differs from C34 **only in the
Android cfg(test) remount fixture**; the sole changed file and equal production
prefix SHA-256 are retained. No production Runtime delta confounds this cohort.
The owner's later mapped-overlay Runtime `7f80b14` is outside these measurements;
Linux deliberately retains the tested C8 pin.

The historical first cohort used `5e57bb19` / Runtime C34, binary
`395db19e3bd610821b63df012b7d8eed6ceb1145b2b4bf08dd83111297aaba43`.
Its initial functional receipts honestly show the then-unsynchronized CLI
license/doc commit and generated Python bytecode. Clean rebuilds produced the
exact same baseline/initial-candidate bytes, allowing those original functional
records to be reused for that historical cohort. These receipts and both full
cohorts remain separate; no samples are spliced, filtered or relabelled as final.

## Scoped correctness and CI

The existing ARM64 Lima/VZ debug VM has real 4096-byte pages and nested KVM.
No new VM, image or package download was needed. The NTP-only clock policy from
`afb57f2` remains active. Fresh final-runner results are:

| Check | Result |
| --- | --- |
| Strict Linux workspace checks and unit tests | Passed |
| Real KVM library/hardware, both transports | 38/38 |
| Original core 4 KiB memory | 77/77 |
| Original CPU extension/state/signal/ptrace | 15/15 |
| Clock consistency, native/transports/workers | 5/5 |
| CLI/PTY, cancellation and host deadlines | 20/20 |
| Declared original application workflows | 48/48 KVM; exact 12-case native reference audited and reused |
| Apple Runtime, Linux stub and Mac all-target compile | Passed; unchanged earlier results reused |
| Frozen iOS ARM64 FFI compile | Passed; not an IPA/device acceptance |
| Final signal changes, Apple stub compile | Passed |

The actual-KVM regression holds the same end across 32 SVCs, yields unchanged
state on expiry, releases at assignment finish and safely destroys an armed
executor. CLI acceptance covers cancellation during compute, raw repeated getpid
and sleep, plus a 200 ms host deadline for each, binary stdio and PTY restoration.

Final Linux source has all six hosted checks passing: ARM64 Linux, amd64 Linux
and macOS build/unit; ARM64/amd64 native software WSI; notices. Shared Runtime C8
has Linux/macOS verify and notices passing; Mac `3a4fb92` build/notices passed,
then `d536c85` paired its test-only Runtime pin. These build/native WSI checks do
not establish KVM execution on amd64 or hardware GPU readiness.

**Broad iPad oracle remains unqualified.** iPad `040b2506` passes check, scope,
memory ABI, notices and guest-musl. Its ARM64-under-QEMU kernel-conformance job
reports **524 passed, 18 failed, 3 ignored**, including source-worker progress,
cancellation and fault cases. The failure list, environment excerpt and job link
are retained. Attribution is unresolved: the pre-hook iPad run stopped earlier at
the Android test, so it cannot prove these 18 failures were pre-existing. This is
not a passing full iPad/release run. The Apple build was cancelled after a later
owner push; the frozen local iOS compile is the available scoped evidence. Newer
owner pins and worktrees are preserved, without rerunning their superseded broad
pipeline solely for this report. See [cloud snapshot](kvm-budget-reuse-20261126/cloud-checks.json)
and [unresolved failures](kvm-budget-reuse-20261126/ipad-qemu-failures.json).

## Final complete paired measurements

The final independent audit passes all **320 executions**, including strict
stderr and original checksums. Eight workloads, one/four workers, native plus
both transports/versions, and three interleaved rounds produce **240 measured
executions**; calibration/warmup bring the raw total to 320. Original tools,
work counts, CPU affinity and separate startup/work clocks are unchanged.
Tracing starts only after the uninstrumented schedule finishes.

Wall-clock medians include process/session startup, in milliseconds:

| Workload | Workers | Native ms | SMCCC before → final ms | MMIO before → final ms |
| --- | ---: | ---: | ---: | ---: |
| cpu | 1 | 54.497 | 71.567 → 70.970 | 72.914 → 72.154 |
| cpu | 4 | 58.769 | 127.131 → 124.247 | 133.486 → 128.072 |
| getpid | 1 | 1.644 | 363.358 → 352.646 | 462.146 → 452.120 |
| getpid | 4 | 1.877 | 648.900 → 616.919 | 824.043 → 802.031 |
| pipe | 1 | 7.154 | 165.023 → 160.234 | 204.658 → 201.204 |
| pipe | 4 | 34.505 | 157.672 → 158.665 | 190.354 → 182.670 |
| futex | 1 | 6.877 | 160.067 → 155.650 | 209.567 → 196.074 |
| futex | 4 | 33.776 | 71.370 → 66.452 | 88.535 → 80.279 |
| mmap | 1 | 1.375 | 143.654 → 136.973 | 161.342 → 154.939 |
| mmap | 4 | 8.692 | 666.298 → 660.766 | 705.073 → 696.907 |
| fork-exec | 1 | 1.843 | 175.903 → 169.454 | 197.979 → 191.476 |
| fork-exec | 4 | 4.603 | 637.117 → 614.747 | 662.440 → 683.430 |
| build | 1 | 3079.773 | 20424.530 → 19577.350 | 22168.238 → 21313.547 |
| build | 4 | 982.997 | 29305.607 → 29100.081 | 29808.610 → 29596.370 |
| node | 1 | 109.772 | 2517.563 → 2457.273 | 2792.766 → 2713.652 |
| node | 4 | 209.750 | 7089.306 → 6836.966 | 7267.964 → 7277.025 |

Final getpid wall medians improve **2.2%–4.9%** across the four configurations.
One-worker GCC improves **3.9%–4.1%**; four-worker GCC improves about **0.7%** in
this cohort. The initial cohort's four-worker GCC instead regressed 1.4%–2.1%;
that complete historical result remains in its own report. Do not combine rounds
or treat the final small four-worker gain as a stable general guarantee.

Final pipe/4 SMCCC wall time regresses **0.6%**, fork/exec/4 MMIO regresses
**3.2%**, and Node/4 MMIO is roughly unchanged (**0.1% slower**). CPU/4 inner work
is 0.4%–2.8% slower although wall medians improve; one-worker Node inner work is
also slightly slower while startup-inclusive wall time improves. Three samples,
nested virtualization, host scheduling and startup/cache effects limit precision
and attribution. The [full final report](kvm-budget-reuse-20261126/performance/report.md)
retains all work/wall/CPU/memory distributions; the
[initial full report](kvm-budget-reuse-20261126/initial/performance/report.md)
retains the superseded candidate without changing its sample rows.

Fifteen **separately instrumented** diagnostics are excluded from those timings.
Original getpid (10,000 calls per task), including startup/teardown, gives:

| Workers | Transport | timer_settime before → final | rt_sigprocmask before → final | ioctl before → final |
| --- | --- | ---: | ---: | ---: |
| 1 | SMCCC | 20,288 → 368 | 40,585 → 745 | 10,298 → 10,806 |
| 1 | MMIO | 20,294 → 470 | 40,597 → 949 | 20,522 → 20,992 |
| 4 | SMCCC | 80,644 → 12,898 | 161,327 → 25,837 | 40,804 → 40,687 |
| 4 | MMIO | 80,664 → 16,402 | 161,369 → 32,843 | 81,343 → 81,086 |

Timer/mask calls fall **about 80%–98%**. Timer creation/deletion remains one per
vCPU. Original SVCs still require their transport exit. Genuine budget expiry can
add checkpoint ioctls, as the one-worker counts show; counts alone do not assign
elapsed time to individual operations. The modest wall gains are consistent with
the earlier [HVC/EL1 sampling](kvm-joined-writes-20261126.md), whose old exact
source scope is preserved. Compute/1 inner work remains about 1.05–1.06× native;
syscall-heavy and GCC workloads remain substantially farther from native. This
nested VM cannot establish a bare-metal performance ceiling.

## Evidence and resource use

[Final raw archive](kvm-budget-reuse-20261126/raw-evidence.tar.gz),
[source scope](kvm-budget-reuse-20261126/source-scope.json),
[offline audit](kvm-budget-reuse-20261126/audit.py) and
[audit result](kvm-budget-reuse-20261126/audit-result.json) retain clean receipts,
all final functional results, counters, the initial negative reproduction,
Apple logs and cloud results. The historical
[initial archive](kvm-budget-reuse-20261126/initial/raw-evidence.tar.gz) remains
unchanged. Across the two separate cohorts there are **640 raw executions and
480 measured samples**, not one merged distribution. The unchanged benchmark
helper and its four previously verified negative controls are reused by hash.
The top-level audit checks both cohorts and the explicit initial/final semantic
difference; a passing scoped audit does not clear the 18 iPad oracle failures.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 docs/validation/kvm-budget-reuse-20261126/audit.py
```

The two raw archives total about 3.2 MiB, excluding images, duplicate rootfs
libraries and runner copies. The immutable 169-file master and every unique
frozen runner/log remain. Hash-checked temporary rootfs cleanup recovered
196,505,600 bytes after the initial cohort and **140,845,056 bytes** after the final
cohort. Earlier obsolete compiler-cache pruning recovered **755,224,576 bytes**,
while preserving current variants. Only this task's completed temporary data was
removed; sources, owner environments and validation evidence remain.

Two integration repairs have separate evidence. Runtime C8 corrects the Android
fixture to use original AOSP bind+remount+RO+nodev flags, and additionally requires
filesystem-wide RO to affect a same-filesystem data bind while an independent
tmpfs stays writable. Native Linux and 40 Linux/40 Mac distro tests agree;
production Android code is unchanged. iPad `040b2506` repairs two real busybox.net
connection failures with bounded retries, hash-before-rename temporary downloads
and an identical pinned Buildroot mirror. Both actual archives match the existing
SHA-256, and all four fault/cache controls pass; the guest-musl CI job then passes.

Network/systemd/Docker and media results retain their earlier source/scope.
Full-product 4 KiB/device release qualification, accelerated GPU with KVM on the
same host, physical PCM/A/V clocks, video, SVE/SME and amd64 execution remain open.
The next Linux product batch is guest desktop/input through the shared Wayland
compositor, Xwayland/Zink, keyboard/pointer and window lifecycle. Start with the
existing VM's X11 software path; native Wayland readiness and real-device GPU
qualification need their own gates. Broad iPad oracle failures also require a
focused source-worker/QEMU investigation before claiming paired release readiness.

Primary API references: [KVM immediate_exit](https://docs.kernel.org/virt/kvm/api.html#the-kvm-run-structure)
and [signal masks](https://docs.kernel.org/virt/kvm/api.html#kvm-set-signal-mask).
The Android fixture follows [AOSP init.rc](https://android.googlesource.com/platform/system/core/+/refs/heads/main/rootdir/init.rc).
