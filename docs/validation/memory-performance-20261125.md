# Core 4 KiB acceptance and native/KVM baseline — 2026-11-25

The core memory gate passes **77/77**, with another **40/40** concurrent permission repeats. The paired performance schedule passes all **702 executions**, including **585 measured samples** and 117 calibration/warmup executions. This is core-memory qualification and a scoped performance baseline; full-product 4 KiB qualification remains **false**.

## Qualified inputs

- Clean Linux build source: `44d6faaec4376f58dea0887a78ad897ae312b66b`. Shared runtime: `d544939a6dc1631dc01bd6af0b881560856c0a8c`.
- Frozen release runner SHA-256: `c6f761bc04334e243e862083773b2e05c49791364d3462f05cede99acc622ef4`.
- Actual 4096-byte ARM64 Linux, kernel `7.0.0-43-generic`, in the existing Mac-hosted nested-KVM VM. Six host vCPUs are available. Native and KVM use the same one/two/four CPUs from `[2,3,4,5]`; the external observer uses CPU 0.
- Native and KVM execute the same original ELF, libraries, Node scripts and GCC/make. Inputs bind tool versions, fixture/controller hashes, build/source receipts, affinity and raw outputs. No application instruction is rewritten and no product counter/service thread is added.
- Implementation commit `0cae06fa747ed9f5dbc38d1971be4c9eb242b259` adds only documentation and offline reporting after the qualified build; no production source changes. Concurrent later FUSE/runtime work is outside this frozen baseline. Consumer pins are recorded in [consumer-pins.json](memory-performance-20261125/consumer-pins.json).

The [build receipt](memory-performance-20261125/trames-memory-build-v9.json), [performance inputs](memory-performance-20261125/performance/inputs.json), [all measured samples](memory-performance-20261125/performance/samples.json), [readable report](memory-performance-20261125/performance/report.md) and [core memory results](memory-performance-20261125/memory/results.json) retain the exact cohort. The measured controller is retained inside the raw archive; the later frozen audit controller only derives CPU totals and Markdown from those same samples.

## Memory fixes and verification

1. Original VFS file mappings had lost their FD/offset capability before KVM import. Owned SharedFile storage now retains the actual physical inode, sparse-window offset, original bytes and lifetime. Logical truncate does not shrink physical storage. Arbitrary unsealed/foreign/composite pointers gain no capability; native physical-size responsibility is explicit in an unsafe constructor contract.
2. siglongjmp legitimately skips rt_sigreturn. The old eight-frame counter limit killed repeated valid fault recovery. Delivery now relies on actual stack/memory checks and keeps its diagnostic counter saturating.
3. A kick during EL1 restore could discard a mailbox-owned pending TLBI flag, occasionally allowing a write after mprotect(PROT_READ). The stopped-restore checkpoint now carries unacknowledged maintenance into the next assignment. A forced-boundary hardware regression catches the original failure; ordinary completed syscall entry adds no check or RUN.

| Check | Result | Scope |
| --- | --- | --- |
| Final 4 KiB core matrix | 77/77 | Native plus SMCCC/MMIO at 1/2/4 workers; every original ELF LOAD has 4096-byte alignment |
| Additional concurrent fault repetitions | 40/40 | Final runner; both transports at 2/4 workers, ten repetitions each |
| Mac HVF memory matrix | 22/22 | Same original ELF, 1/4 workers; runtime `cd07862`, before the later unsafe API contract/test-only refinement |
| CPU instruction/state/ptrace matrix | 15/15 | Native and both KVM transports, 1/4 workers; V8 runner `bf85e939…`, runtime `cd07862` |
| Declared application workflows | 48/48 KVM | V8 runner; exact 12-case native reference reused and independently audited |
| CLI/PTY frontend checks | 12/12 | Six checks per transport with V8 runner |
| Software PCM playback/capture | 4/4 | V8 runner; no physical-device claim |
| 4 KiB-only Vulkan/KVM buffer aliases | 2/2 | Both transports, software Vulkan, original bytes in both directions and adjacent read-only sentinels |
| Final compile checks | Passed | Strict Linux/macOS checks and iOS ARM64 FFI compile; affected shared-memory/VFS/signal tests pass |

The V8 regressions remain bound to their original receipt. V9 changes that followed were the explicit unsafe physical-owner contract, sparse-window tests and explanatory comments; affected Linux/macOS/iOS paths were checked. The final 77-cell gate, 40 repetitions and performance schedule reexecute the final V9 binary. Earlier systemd/Docker/network and physical graphics evidence remains at its existing scope.

Historical failures are retained in the compressed archive: the initial mapped-file import failures, the subsequent 74/77 permission-race result and the first Node pilot missing Ubuntu externalized builtin scripts. Expectations were kept intact. Final execution uses the original packaged Node builtins, not substitutes.

## Performance observations

Table values are median inner-work milliseconds over five samples. CPU/memory/syscall/Node cases keep work per application thread fixed; make keeps twelve source units fixed with -j1/-j2/-j4, while IPC always has two application threads. Ratios compare matched configurations; only the fixed-total-work cases directly measure that form of scaling. Ratios greater than one are slower than native.

| Workload | Workers | Native ms | SMCCC ms | MMIO ms | SMCCC/native |
| --- | ---: | ---: | ---: | ---: | ---: |
| cpu | 1 | 55.452 | 59.538 | 58.549 | 1.074 |
| cpu | 4 | 60.209 | 92.881 | 90.997 | 1.543 |
| memory | 1 | 15.859 | 18.329 | 19.054 | 1.156 |
| memory | 4 | 30.087 | 178.163 | 172.444 | 5.922 |
| getpid | 1 | 1.138 | 364.176 | 470.919 | 320.124 |
| getpid | 4 | 1.344 | 639.727 | 823.044 | 475.867 |
| build | 1 | 3140.612 | 24209.845 | 26114.373 | 7.709 |
| build | 4 | 1051.737 | 33177.249 | 33762.464 | 31.545 |
| node | 1 | 28.028 | 53.188 | 52.909 | 1.898 |
| node | 4 | 36.104 | 139.026 | 168.085 | 3.851 |

Single-worker dependent integer work takes about **7.4%** longer than native in this VM. This does **not** establish general near-native behavior: tiny getpid is about **320×** the native inner time, and the GCC/make workload is about **7.7×** at one worker and **31.5×** at four. Compiler sources/object checksums match; poor parallel scaling is an optimization target, not a passing speed claim.

Node illustrates why startup must remain visible: one-worker inner work is about **1.9×**, but total wall time is about **32.2×** native; four-worker total wall time is about **45.7×**. Prefault/setup and worker-local translation admission also matter for memory work. The complete report retains both clocks, CPU time, context switches, faults and sampled RSS/PSS distributions.

These results are nested virtualization on a shared Mac, with warm host page caches and an external 250 ms memory observer. Affinity does not pin physical Mac cores; host scheduling/thermal/I/O activity can perturb results. Setup/RAM prefault is outside the inner region, while first-use worker translations may remain inside. RSS/PSS snapshots can miss short peaks and RSS duplicates shared mappings. Idle CPU totals include startup. No bare-metal ceiling, steady-idle CPU floor or hardware-GPU performance is inferred.

## Separate instrumented diagnosis

The [diagnostic results](memory-performance-20261125/perf-diagnostic/results.json) use Linux perf tracepoints after all primary samples finish. Their times are excluded from the baseline. Counts below include startup and teardown for 10,000 getpid iterations.

| Lane | KVM exits | ioctl calls | timer_create | timer_delete | rt_sigprocmask |
| --- | ---: | ---: | ---: | ---: | ---: |
| native | 0 | 0 | 0 | 0 | 5 |
| smccc | 10498 | 10590 | 10148 | 10148 | 40601 |
| mmio | 10500 | 20517 | 10146 | 10146 | 40593 |

Code inspection and these counts agree: the current Deadline guard creates/deletes a host timer around each execute call, producing approximately one pair per guest syscall. This is a concrete next optimization target. The counts do not establish how much elapsed time each layer consumes. MMIO performs an extra completion RUN ioctl; that does not imply twice as many actual guest exits.

Next performance work should retain a same-worker timer/budget, preserve safe cancellation/checkpoint behavior, and then profile multi-worker mapping synchronization and process-switch costs. Do not add a coordinator, stronger ordinary-memory ordering or application-code rewriting. Recompare affected workloads against this baseline and retain the original cohort. Named-environment persistence and the remaining Linux device providers remain subsequent product work.

## Offline audit, CI and disk

```sh
PYTHONDONTWRITEBYTECODE=1 python3 docs/validation/memory-performance-20261125/audit.py
```

The [offline audit](memory-performance-20261125/audit-result.json) checks the compressed raw records, source/build/fixture bindings, all measured outputs, original memory outputs, thread repetitions, application artifacts and separate counter logs. The [negative tests](memory-performance-20261125/performance/audit-negative-tests.json) reject altered raw output, missing samples, changed planned work and internally consistent results that differ from native. No benchmark or hardware test reruns during reporting/audit. Executables and rootfs/library copies are excluded from the repository archive.

The ordinary Linux CI has a native-only 4 KiB reference. The labelled real-KVM workflow requires all 77 cells, and the separate performance workflow retains repeated raw evidence and audits it. Hosted builds and software WSI are explicitly not hardware passes. Cloud status is retained in [cloud-checks.json](memory-performance-20261125/cloud-checks.json).

For this cohort, shared runtime Linux/macOS CI, all three Linux product build/unit jobs, both native software WSI references and the Mac consumer build pass. iPad memory-abi and guest-musl pass; its apple-build was cancelled and is not counted as a cloud pass. The local final iOS ARM64 FFI check passes. Later independent FUSE consumer updates do not change these frozen qualification identities.

The [integration record](memory-performance-20261125/runtime-memory-integration.json) preserves the concurrent owner frame/delivery rewrites, all 274 staged paths and 1,380 unrelated files while advancing the validated runtime base. Later independent runtime commits are not included in these qualification claims. The [cache cleanup record](memory-performance-20261125/memory-build-cache-cleanup.json) removes only an inactive temporary Apple build cache (about 8.6 GiB), preserving signed binaries and evidence; host free space returned to about 51 GiB. Existing VMs and guest release caches were reused; no new VM image was downloaded.

## Remaining release gates

Full-product 4 KiB qualification remains **false** until same-host KVM plus accelerated GPU, image/FD/fence ownership and synchronization, physical PCM recovery/clocks, A/V synchronization and hardware video have been exercised. The GPU-capable UTM VM remains a native graphics reference without KVM. SVE/SME and amd64 need their own complete state/ABI hardware cohorts.
