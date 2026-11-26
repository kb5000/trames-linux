# Joined-epoch KVM memory publication — 2026-11-26

In this nested VM, selected median wall times fall about **4%–8% for GCC/make** and **14%–24% for the Node workload**. Getpid remains approximately unchanged. This batch removes the writable-page walk after each KVM execute call. Writable provider/code/allocation memberships are tracked at mapping changes and published once per joined Runtime epoch, using the existing shared APIs and the same timing contract as HVF. Ordinary private pages keep their original compact metadata layout. No shared Runtime production code, application instruction, coordinator, helper thread or ordinary-memory barrier is added.

## Frozen sources

The comparison keeps Runtime `a37a84d4b3c66f21ddb581f8f4b0427f50793e0d`, shared CLI `da18836903d8e37bbb92cc32cc7ec78f8cf457bc`, the Rust release configuration and original workload/tool bytes fixed. Later independent shared Runtime work is outside these measurements.

- Baseline: Linux `88b6d8505ad51a5a0d96fb10ba4e6666367eea0c`, runner SHA-256 `d9641f0fbd8af8b2ef04b4e854efebbdbee91329af0e0b8be49a10f2895d5378`.
- First candidate: `6259a61e3e2f98311fcf9018439d206051599d8b`. All functional checks pass; its full performance run is retained separately.
- Final compact candidate: `c0955881e5554c659f8e15dc1c1c4cf129f02607`, runner SHA-256 `345289fd15f4fad74ccc971ccfcefbd725919c12f049e7c3c914c06ee77590e4`.

Only the KVM engine implementation and its real-hardware regression change. Shared Apple paths use the unchanged Runtime and their previous source-bound checks; the Linux product's platform builds are checked separately. Reporting changes do not rerun guest workloads.

## Memory lifetime and publication

The old engine visited every cached writable page on every execute return, repeating address/frame lookup and conservative publication even for an unchanged private view. The new engine records only actual publication resources:

1. Root-frame bitmaps group file/code dirty membership using `HardwareWriteFrame::publication_word` and `View::mark_hardware_write_word`.
2. Allocation epochs retain reference-counted membership, advancing each original epoch once per joined round.
3. Ordinary canonical RAM needs the existing private-dirty marker and retains no per-page publication token.

Each stopped address space keeps its own metadata alongside its translation root. Mapping-generation changes revoke stale leaves and prepare a replacement write set before publishing it. A failed cold admission leaves a retryable set; the last accepted version is not advanced. Newly exported writable pages are already published by Runtime's original cold export, before direct hardware stores. A later joined round clears publication and changes the existing view token, causing re-publication before reentry. Repeated short SVCs in the same round require no page walk.

Tokens retain checked metadata and allocation epochs, not physical RAM ownership. Cached pages still hold weak physical aliases. Protection, unmap, remap, backing versions, sparse memory, revocable byte borrows and retirement continue through the original mapping and custody boundaries. The change introduces no additional syscall policy or hot byte-access check.

## Sampling findings and limits

Before implementation, independent Linux perf runs sampled the frozen baseline with software `cpu-clock`. These diagnostics are excluded from all primary timing distributions. Initial ordinary reports displayed only host samples, leaving most recorded time unrepresented. They remain retained with that limitation. The corrected `perf kvm --host --guest report` accounts for the complete distribution.

For 100,000 getpid calls, the corrected run attributes 77.53% of samples to guest PC `0xffffffffffff8b04` and 14.29% to `0xffffffffffff8860`. The exact baseline ELF symbols/disassembly place these at the private EL1 HVC return and FPCR restore, respectively; timer setup is about 7%–8% of the complete run. This is sampled attribution around virtual-machine exits and state transitions. It does not mean the branch instruction itself consumes 77% of execution, or predict a bare-metal cost split. Sampling skid, guest-mode attribution and nested virtualization all limit interpretation.

The [upstream perf KVM documentation](https://android-kvm.googlesource.com/linux/+/refs/heads/attic/for-android/pkvm-modules/tools/perf/Documentation/perf-kvm.txt) describes the host/guest report controls. The installed perf also emits an `Invalid HEADER_EVENT_DESC` metadata warning. Sample rows and the complete normalized distribution remain readable; these reports are used only to locate transition points, not to apportion precise instruction latency. The test machine is still the same Mac-hosted nested ARM64 KVM VM, with actual 4096-byte Linux pages and kernel `7.0.0-43-generic`. Native affinity does not bind physical Mac cores. Raw stack captures were removed after symbol reports; no guest stack bytes are published.

## Functional and performance results

| Check | Result |
| --- | --- |
| Strict workspace Clippy and normal tests | Passed |
| Real KVM library/hardware matrix | 36/36, both transports |
| 4 KiB original core memory matrix | 77/77 |
| Original CPU extension/state/ptrace | 15/15 |
| Declared application workflows | 48/48 KVM; independently checked native 12-case reference reused |
| Original libfuse | 108 functional plus 36 shutdown checks |
| 4 KiB software Vulkan/KVM buffer aliases | 2/2; original bytes in both directions and read-only neighbors |

The new real-KVM regression exercises three consecutive SVCs per View, four joined writebacks, switches to another address space, protection and replacement of the original backing. No full network/systemd/Docker or physical-device corpus is repeated.

Each full interleaved schedule retains 240 executions, including 180 measured samples and 60 calibrations/warmups. The first-candidate schedule passes. The final schedule's original audit rejects one baseline build sample because make emits a clock-skew warning; its exit and object checksum match native. The complete failed result remains in [original-performance](kvm-joined-writes-20261126/original-performance/report.md).

A scoped [supplemental build round](kvm-joined-writes-20261126/retry-performance/report.md) runs native and both transports/versions together, with calibration and warmups: ten executions. The qualified derived view replaces the entire affected five-lane measured round, including its four otherwise valid samples. Source rows, repeats and log bytes are retained unchanged under explicit origin metadata; the fixed selection rule is independently audited. No timing-based sample selection or stderr exception is allowed. The supplemental build's executable/library hashes match the corresponding original inputs; its rootfs omits unused Node files.

There are 490 retained raw executions across both candidates and the supplement. Each candidate's qualified view contains 240 selected executions/180 measured samples; the final view is explicitly a paired-round reconstruction. These are two separate candidates, not six independent repeats of one candidate. The warning alone does not identify whether virtual-clock synchronization or the guest wall-clock path caused the mismatch; wall-clock/stat consistency remains a separate investigation.

The final candidate's wall-time medians are below. Work/CPU/resource distributions remain in the [complete report](kvm-joined-writes-20261126/performance/report.md); first-candidate data remains in the [initial report](kvm-joined-writes-20261126/initial-performance/report.md). Lower ratios are faster on this configuration; these small samples do not establish a universal speedup.

| Workload | Workers | Native wall ms | SMCCC before → after ms | SMCCC after/before | MMIO after/before |
| --- | ---: | ---: | ---: | ---: | ---: |
| build | 1 | 3110.366 | 22404.055 → 20959.300 | 0.936 | 0.918 |
| build | 4 | 983.406 | 30572.572 → 28991.466 | 0.948 | 0.959 |
| node | 1 | 111.021 | 3431.567 → 2619.490 | 0.763 | 0.788 |
| node | 4 | 207.913 | 8263.206 → 7112.902 | 0.861 | 0.864 |
| getpid | 1 | 1.641 | 372.548 → 373.335 | 1.002 | 0.983 |
| getpid | 4 | 1.877 | 668.743 → 670.604 | 1.003 | 0.985 |
| memory | 1 | 17.094 | 200.428 → 197.641 | 0.986 | 0.978 |
| memory | 4 | 34.098 | 1050.238 → 991.272 | 0.944 | 0.954 |

The original workload sources, native reference semantics, compiler object checksums, tool/library hashes and build receipts are retained. Measurements separately report inner work, total wall time, CPU, faults, switches and sampled RSS/PSS. Setup/prefault is outside inner work; first-use translations can still occur inside it. Idle and device capability claims are outside this batch.

The first candidate's memory/4-worker samples have substantial spread, including a slower SMCCC median. Its complete distribution remains separate from the compact candidate. The compact revision removes private-page token storage and the temporary full-binding vector; these two runs do not separately isolate that layout change from scheduling and first-use-translation variation. It preserves the original `Page` layout and only retains metadata for real publication resources. Do not replace one cohort's binary identity or favorable samples with the other cohort's results.

No bare-metal ceiling or general near-native syscall claim follows from this VM. The previous timer lifecycle counters retain their original scope; per-execute arm/disarm and signal masks are unchanged here. Remaining work includes measuring execution-budget lifetime and private EL1 state handling while preserving full SIMD/FP, signal, ptrace, migration and cancellation semantics.

## Audit and resource use

```sh
PYTHONDONTWRITEBYTECODE=1 python3 docs/validation/kvm-joined-writes-20261126/audit.py
```

The [offline audit](kvm-joined-writes-20261126/audit-result.json) preserves the original failed audit, verifies its supplemental matched round and the exact reconstructed selection, then verifies both qualified distributions, source/build/tool/fixture bindings, raw logs, original core/CPU outputs, application artifact fingerprints, FUSE activation/shutdown and buffer tests. It reuses the unchanged frozen benchmark auditor and its four previously verified negative-evidence cases; their exact files are pinned by the new index. Two additional provenance checks reject a partial-round selection and a changed derived timing. A repository checkout supplies those earlier frozen helpers. No benchmark or hardware work reruns during audit.

All three hosted build/unit jobs pass for final source `c095588`; they validate portability and software checks, while the real KVM/graphics-buffer cells remain separately identified.

Source and adapter identity are recorded in [source-scope.json](kvm-joined-writes-20261126/source-scope.json), the observed guest PCs in [profile-symbols.json](kvm-joined-writes-20261126/profile-symbols.json), and independent cross-platform source checks in [cloud-checks.json](kvm-joined-writes-20261126/cloud-checks.json). Raw archives exclude executables, rootfs/library copies and perf stack captures. The first compile-time MemoryError conversion diagnostic is retained; the corrected source passes the scoped checks.

All existing VM images, tool bundles and release caches are reused. Completed runner duplicates are verified by SHA-256, mode and ownership before hardlinking to the cohort's identical frozen runner; every retained result path continues to resolve to the same bytes. Rootfs duplicates are removed only after their immutable files match a retained copy and their mutable work directories are empty. The [final cleanup](kvm-joined-writes-20261126/final-cleanup.json) reclaims another 164 MiB of identical/subset rootfs inputs after archiving. Its checkpoint leaves about 0.80 GiB free in the guest. Current source work, unique qualified runners and raw results remain retained.

Full-product 4 KiB qualification remains false pending the existing device gates. GPU, physical PCM/A/V synchronization, hardware video, SVE/SME and amd64 execution are not newly qualified by this backend-memory batch.
