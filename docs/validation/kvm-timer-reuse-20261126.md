# KVM timer reuse and shared FUSE integration — 2026-11-26

The Linux backend now retains one deadline timer per vCPU instead of creating and deleting it around every execute call. The paired getpid microbenchmark improves by **1.6%–6.5%** in this nested VM. The remaining gap to native is substantial. Shared Runtime/FUSE is updated, and the host-directory frontend now exposes the Runtime FUSE device; all **144** stock libfuse functional and shutdown checks pass.

## Inputs and delivered commits

All active repository/remotes, submodule URLs, package repository metadata and runtime-version records now use `project-trames`. Historical validation records retain their original identities. Runtime and CLI remain separate shared repositories; Linux is the independent sibling repository `/Users/main/dev/trames-linux`.

| Input | Identity |
| --- | --- |
| Shared Runtime for every new Linux measurement | `a37a84d4b3c66f21ddb581f8f4b0427f50793e0d` |
| Shared CLI | `da18836903d8e37bbb92cc32cc7ec78f8cf457bc` |
| Runtime refresh, old deadline implementation | `cadd2ac6c3ea2b1da3fd8f17eebf6907b44039c0` |
| Timer implementation and signal regressions | `d9b4803ede684e2355ff3b079ee67d541ac4098e` |
| Interleaved measurement controller | `566baf3031920c2a2c8311b8686b8d86f920d542` |
| Host-directory `/dev/fuse` attachment | `88b6d8505ad51a5a0d96fb10ba4e6666367eea0c` |

The frozen before/after/final-front-end runner SHA-256 values are respectively `7fffbd8dfac0640b779d2e23f2e3d917adbf73920e54c370112def55c58a9309`, `af26f5c6d34cdab5f9dae69a7671e7276895dde711b2b9784e7a67051164060c`, and `d9641f0fbd8af8b2ef04b4e854efebbdbee91329af0e0b8be49a10f2895d5378`. Their complete source/build receipts are inside the raw archive.

Unlike the previous D544 memory baseline, both sides of this timer comparison use the same A37 Runtime. Rust toolchain, build flags, original workload sources, immutable rootfs bytes and application-tool versions match. Production differences are limited to three KVM implementation files; the subsequent frontend fix changes four cold rootfs-setup lines. Later benchmark changes only derive reports from retained samples. See [source scope](kvm-timer-reuse-20261126/source-scope.json) and [consumer pins](kvm-timer-reuse-20261126/consumer-pins.json).

## Deadline lifetime and cancellation

Each creator-thread executor owns its timer and unique identity until teardown. `timer_create`, `timer_delete`, identity allocation and creator `gettid` leave the per-execute path. The guard still arms/disarms the timer, publishes the original KVM mapping while signal delivery is blocked, drains a pending notification and restores the original signal mask. No application instruction rewriting, coordinator, helper thread, production instrumentation or stronger ordinary-memory ordering is introduced.

Timer identity rejects a notification from a destroyed vCPU. A retained timer also needs to distinguish an old queued expiry from a new arm: on an actual timer signal, the handler checks the timer's remaining time before touching `immediate_exit`. An explicit worker kick remains independent of that check. The handler preserves host errno. The remaining-time query occurs only on timer delivery, not ordinary guest SVC entry.

Linux documents the [timer state/disarm API](https://man7.org/linux/man-pages/man2/timer_gettime.2.html), the [unspecified fate of pending notifications after deletion](https://man7.org/linux/man-pages/man2/timer_delete.2.html), and the [async-signal-safe timer query and errno handling](https://man7.org/linux/man-pages/man7/signal-safety.7.html). Six real Linux signal tests cover expiration, disarm, explicit kick, stale expiry after rearm, a retired timer's real notification and nested-arm rejection. All pass without a timer service thread.

## Performance evidence

The existing ARM64 KVM VM has actual 4096-byte pages and kernel `7.0.0-43-generic`. Native and KVM use matched affinity, original ELF bytes and work. Two six-workload macro runs retain **108 measured / 144 total executions each**. A separate five-lane interleaved micro run retains **150 measured / 180 total executions**, shuffling native plus baseline/candidate SMCCC/MMIO each round. All **468 executions** match native workload results; 366 are measured samples.

Medians below are inner getpid work in milliseconds, five samples per lane. Work per application thread is fixed at 10,000 calls; four workers perform four times the total work.

| Workers | Transport | Before ms | After ms | After/before |
| --- | --- | ---: | ---: | ---: |
| 1 | SMCCC | 491.869 | 459.782 | 0.935 |
| 1 | MMIO | 608.459 | 592.671 | 0.974 |
| 4 | SMCCC | 795.763 | 768.252 | 0.965 |
| 4 | MMIO | 1032.733 | 1016.529 | 0.984 |

Native is 1.366 ms at one worker and 2.689 ms at four in this run. The candidate still takes about **336× / 286×** native inner time for SMCCC. This batch removes one concrete overhead source; it does not establish near-native syscalls or an overall application speedup.

The sequential macro measurements show material shared-host drift: native build time changes from roughly 3.12 s to 4.20 s at one worker, and four-worker KVM builds also become slower. All data is retained in the [before report](kvm-timer-reuse-20261126/before-performance/report.md), [after report](kvm-timer-reuse-20261126/after-performance/report.md), and [interleaved report](kvm-timer-reuse-20261126/paired-micro/report.md). Do not select only favorable macro samples or interpret the timer change as the cause of all differences. Nested scheduling, host activity and short distributions limit attribution, including differences in pure compute. Idle CPU totals include startup and do not establish a steady-idle floor.

### Separate traced diagnostics

Linux perf tracepoints run after primary timings. The fifteen instrumented runs are excluded from the distributions. Representative one-worker getpid counts include startup/teardown:

| SMCCC event | Before | After |
| --- | ---: | ---: |
| timer_create | 10153 | 1 |
| timer_delete | 10153 | 1 |
| gettid | 10160 | 8 |
| timer_settime | 10153 | 20290 |
| rt_sigprocmask | 40621 | 40589 |
| timer_gettime | 0 | 4 |
| ioctl | 10411 | 10371 |

Both transports show one timer per vCPU: four-worker runs drop from about 40,350 create/delete pairs to four. Signal masks and per-execute arm/disarm remain. The disarm changes from timer deletion to `timer_settime(0)`, so settime counts increase. No claim of eliminating all deadline-related host calls is made. Counts alone do not apportion elapsed time among KVM transitions, signals, memory publication and runtime work.

## Functional checks and FUSE repair

| Scope | Result |
| --- | --- |
| Retained timer signal tests | 6/6 |
| Real KVM library/hardware checks | 34/34, both transports |
| Strict Linux workspace checks and normal tests | Passed |
| Original core 4 KiB matrix | 77/77 |
| Original CPU extension/state/ptrace matrix | 15/15 |
| Declared application workflows | 48/48 KVM; exact native 12-case reference independently audited and reused |
| CLI/PTY | 12/12 on timer runner, 12/12 after frontend device fix |
| Original libfuse | 12 configurations; 108 functional plus 36 shutdown checks |
| Mac strict checks and frozen iOS ARM64 FFI compile | Passed with the paired shared Runtime/CLI |

The new Runtime contains the shared FUSE implementation, but Linux `--rootfs DIR` recreated a small `/dev` and omitted `/dev/fuse`. Stock libfuse consequently failed before mounting. The frontend now bind-mounts the actual shared Runtime device into that private `/dev`; creating a nominal device file would not preserve its connection provider. No host kernel FUSE daemon or host filesystem mount is substituted.

The unchanged stock lowlevel, writeback and highlevel tools/workload pass at 1/4 workers with both transports. BusyBox files, mmap, copyout, metadata, AIO, copy, transfer, cached I/O, io_uring, unmount and daemon shutdown all pass. Writeback logs confirm actual activation. All initial twelve missing-device failures remain in the archive. Stock tool/image hashes and commands are bound in each input record; no image or tool download was needed.

The initial Mac check failed on a stale build-time guest CLI manifest. Reuse proceeded only after the complete builder source hash and exact executable hash matched; the failure and final passing log are retained. The final iOS check used a frozen checkout and clean dependency copies because the owner's separate Runtime worktree continued changing. That work was preserved.

The migrated iPad Linux cloud job exposed a missing test-machine dependency: the original ALSA plugin could not find `alsa/asoundlib.h`. Commit `62c3c27c` adds `libasound2-dev` to its existing dependency step. YAML parsing, the ALSA ABI oracle and both software PCM tests pass. This CI-only fix uses `[skip ci]` to reuse unchanged broad checks; the historical failed job is retained and is not relabelled as a cloud pass. Cloud source-check states are recorded in [cloud-checks.json](kvm-timer-reuse-20261126/cloud-checks.json).

The qualified Linux source `88b6d85` passes all three hosted build/unit jobs (ARM64 Linux, AMD64 Linux, macOS) and both native software WSI references. The shared Runtime Linux/macOS checks, all three shared CLI jobs and the Mac consumer build also pass. Hosted/software results remain separate from the real KVM matrix. The historical iPad cloud failure and pending Apple frontend build are recorded at their actual status.

No unchanged full network/systemd/Docker, physical-device or graphics cohort was rerun for this reporting batch. Prior evidence remains bound to its original source. SVE/SME and amd64 execution remain unqualified; full-product 4 KiB qualification remains false pending the device gates.

## Audit, disk and next work

```sh
PYTHONDONTWRITEBYTECODE=1 python3 docs/validation/kvm-timer-reuse-20261126/audit.py
```

The [offline audit](kvm-timer-reuse-20261126/audit-result.json) verifies build/source/fixture bindings, all distributions, core outputs, application artifact fingerprints, timer counters and FUSE logs. Four negative checks reject changed raw output, a missing sample, a self-consistent worker-count change and a self-consistent checksum that differs from native. No VM or workload runs during audit.

Raw evidence compresses to about 0.8 MB; rootfs images, executable copies and debug caches are excluded. A [cleanup record](kvm-timer-reuse-20261126/duplicate-rootfs-cleanup.json) verifies all 169 files before deleting only the duplicate after-measurement rootfs, reclaiming about 126 MiB while retaining the identical before rootfs and all runners/logs. Existing VM/build caches and stock tools were reused. The VM still has only about 0.6 GiB free, so further broad builds/images require reclaiming known obsolete outputs first. Host free space remains about 50 GiB. The active owner's source and validation work remain untouched.

Next performance work should profile per-execute signal masking, mapping publication and process-switch costs, then retain a safe same-worker execution budget across fast SVCs where the evidence supports it. Keep real expiration, cancellation, retirement and stale-notification tests intact. Shared CLI named-environment persistence and remaining accelerated GPU/audio/video providers remain separate product batches.
