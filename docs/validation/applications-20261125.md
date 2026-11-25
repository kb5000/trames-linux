# First automated application acceptance — 2026-11-25

The acceptance system works, and the current Linux product does **not** pass the
application matrix. Native Linux passes 12/12 workflows. KVM passes 7/48 cells
across two transports and one/four workers; 41 cells fail. These are 12 workflow
specifications in four KVM configurations, not 48 independently supported apps.
The common Python subprocess/socketpair/thread/fork workflow passes in all four;
other isolated passes vary across runs/configurations and do not prove stability.

The shared [acceptance architecture](../architecture/acceptance.md) describes
layering, strict evidence, source-bound reuse and the next adapters/gates.

## Inputs and environment

- Product executable source: `bc3af168b11014fb55b4ce185de87cfdc269dc23` plus the
  uncommitted test-only controller; production Rust is unchanged by this batch.
- Runtime production source: `5b12cc9e69ff12f7eac6d0b3bb78f5a5218e38da` plus the
  new test-only harness. This is development diagnostic evidence, not a release
  acceptance of a clean final product build. Build provenance preserves dirty
  state and exact production input/binary digests, without relabelling them.
- Original Ubuntu ARM64 distribution binaries, including Python 3.12, Node
  18.19.1 and GCC 13; exact packages and per-program bytes are recorded in
  [bundle metadata](applications-20261125/bundle.json).
- Existing Lima/VZ VM: ARM64 Linux 7.0.0-43-generic, actual 4096-byte pages and
  nested KVM. Timings include startup/setup and do not qualify bare-metal
  performance, latency or a native speed comparison.
- Frozen rootfs tree SHA256:
  `e430c11d2b90acea8f070043a6d1dedef55d461b499d3da9ab5dac34b114237e`.
  Full bundle retained outside Git at
  `target/local-ci/io/acceptance-20261125/bundle.tar.gz` in the iPad development
  workspace (archive SHA256 `af9126f4a8855759611c830c9027e5551628b69b1fc5e752b2028a4cd4dc2274`), and at `/home/main.guest/trames-acceptance-bundle-v2` in the VM.
- Baseline harness revision: `7a8c949e01e417c036c82744a259e9de88025cb2`,
  fingerprint:
  `8863dd24f967e6584b7bf7d8e1729b6170784ff7607ae8500a473f3c30d895cf`.
  This matches the exact committed common/run/native scripts used for all raw
  evidence below. The controller and report contain no production code.

## Matrix and evidence

The [coverage table](applications-20261125/trames-acceptance-matrix-qualified/report/report.md)
contains all 60 native/KVM cells. The independently audited
[summary](applications-20261125/trames-acceptance-matrix-qualified/report/summary.json)
and [JUnit](applications-20261125/trames-acceptance-matrix-qualified/report/junit.xml)
retain counts, selected cases, raw plans and build/environment provenance.
Per-lane `cases/ID/` directories preserve original stdout, stderr, result JSON and
actual deterministic output files. The
[build receipt](applications-20261125/trames-acceptance-build-final.json) identifies
the executable and records the real successful build, toolchain and source inputs.

| Configuration | Passed | Failed | Missing/reference/timeout |
| --- | ---: | ---: | ---: |
| Native Linux | 12 | 0 | 0 |
| KVM SMCCC, 1 worker | 1 | 11 | 0 |
| KVM SMCCC, 4 workers | 2 | 10 | 0 |
| KVM MMIO, 1 worker | 1 | 11 | 0 |
| KVM MMIO, 4 workers | 3 | 9 | 0 |

The two earlier native-fixture issues were fixed before this baseline: copy the
actual GCC `cc1`/collect2/libexec dependencies and compare SSH public-key fields
independently of their comment formatting. No application assertion was weakened
for Trames; the same scripts then run on both native and KVM.

## Observed product blockers

1. Ordinary host-mounted files written by one command frequently return EIO on
   subsequent reads. This affects shell pipelines, cmp, tar/base64, Python file
   reads, Git metadata, SSH private-key reading and Makefile reading. Some cells
   pass in other runs/configurations; this needs a shared file/I/O root-cause fix
   and repeated validation. This record does not claim a proved mmap/permission/
   filesystem root cause from stderr alone.
2. The Node workflow terminates with `Illegal instruction`. CPU feature exposure,
   system-instruction and JIT/cache interactions need a separate minimal trace;
   this record does not assume which instruction caused it.
3. A single successful text/copy/curl workflow is insufficient for general tool
   support. curl here uses file://, not HTTP/TLS; ssh-keygen is not SSH login;
   the Python SQLite step currently fails earlier at ordinary file reading.

These gaps predate the harness. This batch changes no production implementation,
adds no production checks/threads, and does not pretend the software matrix is
passing because the harness/native CI is passing. Application acceptance remains
an explicit required-hardware workflow that exits nonzero on these failures.

## Reuse and harness verification

- The initial ten acceptance-harness regression tests passed on macOS and ARM64 Linux. They
  cover changed inputs, failed/missing/forged output, missing/modified artifacts,
  manifest/escaping paths, actual process-group timeout, abnormal exit, binary
  receipt mismatch, immutable prior output and independent report re-audit.
- Native rerun with identical inputs reused all 12 passing cases; no native
  software was re-executed. [Reuse summary](applications-20261125/trames-acceptance-native-reused/results.json).
- A same-input SMCCC/one-worker retry reused its one passing workflow and
  actually re-executed all 11 failures. They remain failures. See
  [retry summary](applications-20261125/trames-acceptance-retry/kvm-smccc-1/results.json)
  and [audited table](applications-20261125/trames-acceptance-retry/report/report.md).
- Python syntax and workflow YAML parse checks passed. Production Rust sources,
  Cargo dependencies and guest CLI inputs did not change; previous functional/
  ABI/media checks remain applicable and were not rerun for test/report edits.

Next batch: first resolve the shared ordinary-file EIO and the Node SIGILL,
then rerun affected cases and stress repetitions. Later profiles add private
HTTP/TLS/SSH peers, package managers, other distributions/libcs, HVF/interpreter/
iPad adapters, broad gVisor/KVM comparison, resources and real media/performance.

The KVM workflow is ready but the repository currently has zero registered
Actions runners. This evidence comes from the local existing KVM VM automation,
not from a cloud hardware job. Hosted native/harness checks are separate;
continuous KVM scheduling and actual release enforcement still need runner/policy
integration. No new permanent runner/service was installed here.

The metadata-hardening follow-ups also bind reuse to root/file ownership, mtime
and extended attributes (including capability/ACL data), and preserve these in
PAX archives. Thirteen harness tests pass on ARM64 Linux; macOS passes twelve
with the one GNU/Linux archive-specific test explicitly skipped. The actual
Linux archive roundtrip preserves the complete tree fingerprint. These changes
affect cache/archive identity only; the historical software baseline above keeps
its exact 7a8c949 harness fingerprint and is not silently reused under a new key.
No production source changed, so software/CPU/media execution was not repeated
for this metadata-only follow-up. Hosted native/harness CI for 7a8c949 passed
on both Linux architectures and the Mac/Linux harness jobs.
