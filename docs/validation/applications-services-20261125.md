# Application fixes and expanded Linux acceptance — 2026-11-25

The previous 7/48 application baseline is historical. After the shared file
capability and ARM64 EL0 access fixes, all 12 basic workflows pass in each of
SMCCC/MMIO at one/four workers: **48/48 KVM cells**, with 12/12 native workflows.
This covers the declared workflows, not all features of these programs.

The expanded IP/systemd/Docker gates are implemented and deliberately remain
red where KVM differs from a passing native reference. Native passes both IP
workflows and both service workflows. KVM passes address/route configuration in
all four configurations. The namespace/bridge workflow completes its operations
but emits unsupported raw-ICMP filter/timestamp socket warnings. Systemd and
Docker still fail their full lifecycle gates; neither is claimed supported.

## Fixes and product behavior

- Disk read caching no longer borrows a retained O_WRONLY descriptor. A read
  requires the existing read access role; the original native read then succeeds
  instead of returning EBADF as guest EIO. No per-I/O fcntl or new lock is added.
- Linux O_PATH metadata operations address the retained original inode, including
  after rename/unlink. Apple paths remain isolated by target cfg. A named host
  mount operand keeps its accepted native position instead of reconstructing it
  from an absent projected-cache entry. Regression tests reproduce both bugs.
- KVM enables Linux's EL0 CTR_EL0/cache-maintenance and WFE permissions using
  compile-time SCTLR configuration, shared by both execution initializers. Node
  originally trapped on CTR_EL0 access. Application/SVC/JIT bytes are not patched.
- Directory import emits contiguous tar payloads even for sparse ELF files.
  Rootfs import uses the existing unpublished staged-population API rather than
  repeatedly publishing every intermediate manifest.
- The Linux shell supports create-only `--store DIR --rootfs DIR_OR_ARCHIVE`,
  reopening with `--store DIR`, and stopped artifact export via `read-file`.
  Provenance lives outside the existing `fs/` store. Host-rootfs execution
  retains proc/sys/devpts and guest devices before entering the host grant.
- `--network off|loopback|outbound|full` selects the existing shared virtual
  fabric explicitly; omission retains the original disabled policy. The new
  network/service profiles use outbound mode. Netlink supports iproute2's
  unnamed NEWLINK feature probe, non-dump AF_PACKET GETLINK, dummy links, and
  route-delete scope NOWHERE.

## Acceptance scope

`network.json` exercises MTU, IPv4/IPv6, JSON link/address/route queries, metric
routes, creation/deletion, netns, veth, bridge membership, ping and cleanup.
`services.json` exercises stock systemd as PID 1, oneshot/notify services,
journal, stop/restart/halt; and default Docker runc/overlay2/bridge/seccomp,
offline image import, exec, DNS, published guest ports, named volume and daemon
restart. Docker's application fixture handles PID 1 SIGTERM and checks exit 0.

The service image is prepared in a disposable chroot/mount/PID namespace and
frozen with package versions and metadata. Docker is not installed or started
on the host. The native service adapter uses systemd-nspawn with isolated
namespaces and a delegated cgroup; explicit outer capabilities/syscalls allow
the stock Docker default security configuration to run. All native cases pass
before a KVM result qualifies. A nonpassing native reference prevents the guest
case from running.

Matrix input freezing prevents concurrent builds from replacing an accepted
runner/helper/manifest. Declared diagnostic consoles retain original bytes and
hashes; required deterministic files, stdout/stderr and exit codes are still
strict. Missing/changed evidence and global infrastructure failure invalidate
passing cache entries. No known failure is xfailed or silently accepted.

## Validation and remaining work

The existing ARM64 Lima/VZ VM has nested KVM and actual 4096-byte host pages.
Timings include setup/startup and are not bare-metal performance evidence.
Shared runtime is pinned to `26384a8746933ea636888f57ac49e62d978a6612`;
CLI remains `078cc9e21ceafd79f631c74bc2f52083d6282a09`.

- Basic applications: 48/48 KVM, 12/12 native. Shell/coreutils, text filters,
  compression, checksums, Python SQLite/processes/threads/fork, Git, curl file,
  SSH key conversion, Node workers/subprocesses, and GCC/Make pass their workflows.
- Original generated-code/file capability/store reopen fixture: 16/16.
- Existing CLI PTY/environment/signal/rootfs acceptance: 6/6.
- Shared runtime full test batches: macOS 2232 passed, 31 explicitly ignored;
  Linux 2229 passed, 15 explicitly ignored. Latest affected VFS batches: macOS 1041 and Linux 1038 passed,
  one ignored each; Linux network 207 passed. Strict clippy and target builds
  are checked on Apple and Linux. Unchanged full CPU/media evidence is reused.
- Acceptance self-tests: Linux 17 passed; macOS 16 passed with the Linux archive
  metadata roundtrip explicitly skipped. Product tests and strict clippy pass
  on both hosts. The Unix socket CLI test requires leaving the macOS sandbox.

The remaining gates must be fixed as complete subsystem batches: raw ICMP
filter/timestamp socket behavior; systemd generator child/sandbox lifecycle;
Docker/containerd startup and cgroup controller/file lifecycle. Diagnostic logs
include generator alarm/protocol failures and missing Docker cgroup control/
memory event paths. These observations are not a proved single root cause.

The KVM workflows are automated locally and dispatchable through labelled
hardware jobs. There is still no registered GitHub KVM runner; this does not
claim continuous hardware CI or an enforced branch/release policy. HVF,
interpreter and physical iPad application adapters remain separate follow-up
work. No Mac/iPad application deployment is part of this batch.

## Reproducible evidence

The final basic application and IP matrices were built from clean product
`3338f735e041249fbc4a0d66072df2a72c6f0d51` and runtime
`26384a8746933ea636888f57ac49e62d978a6612`. The actual
[clean build receipt](applications-services-20261125/build-clean.json) records
both clean flags, source input digests, toolchain, successful build argv and
binary SHA256 `faea82dd03da357f26dad259063509ba08e19b286fc6d02a003d558424137f81`.

| Profile | Native | KVM SMCCC 1/4 workers | KVM MMIO 1/4 workers | Evidence |
| --- | --- | --- | --- | --- |
| Basic applications | 12/12 | 12/12 each | 12/12 each | [audited table](applications-services-20261125/applications/relocated-report/report.md) |
| IP address/route | passed | passed / passed | passed / passed | [audited table](applications-services-20261125/network/relocated-report/report.md) |
| IP netns/bridge/ping | passed | failed / failed | failed / failed | same network report |
| systemd PID 1/services | passed | failed / failed | failed / failed | [diagnostic table](applications-services-20261125/services-diagnostic/relocated-report/report.md) |
| Docker lifecycle | passed | failed / failed | failed / failed | same services report |

The service diagnostics use the complete final implementation before committing
the pin manifest; their [receipt](applications-services-20261125/build-services-diagnostic.json)
truthfully retains dirty development provenance. They are failure diagnostics,
not release qualification of a clean build. All four final Docker cells stop
at managed containerd startup timeout. Earlier diagnostic runs also reached
container creation and exposed missing cgroup control/event files. All four
final systemd cells stop at generator child alarm/protocol failure. These gates
must pass before claiming systemd/Docker support.

Each matrix directory preserves its full plan, result JSON, raw stdout/stderr,
console where declared, deterministic artifacts, diagnostics, JSON summary and
JUnit. Passing evidence was independently audited again after relocation.
The [file/JIT result](applications-services-20261125/file-jit/results.json),
[CLI result](applications-services-20261125/cli-results.json), and platform
test/clippy logs under `applications-services-20261125/checks/` remain separate
from application compatibility counts.

Full frozen images are retained in the iPad development workspace at
`target/local-ci/io/acceptance-fixes-20261125/`, outside Git. PAX archives preserve
numeric owners, xattrs and ACLs. [CLI metadata](applications-services-20261125/cli-bundle.json)
and [service metadata](applications-services-20261125/services-bundle.json)
record the exact rootfs trees and package versions. Archive SHA256:

- `cli-bundle.tar.gz`: `6fe698837fd8147caf1e50bc5f3aed77c9893b893c0c2be849a9ceecb426562d`
- `services-bundle.tar.gz`: `31c6fdde1742bb3ba7931adcab758f30e7e6a0450fa07368168bb5924c8d028b`

Clean-commit hosted checks passed: [shared runtime](https://github.com/kb5000/trames-runtime/actions/runs/45550128461),
[native IP/application references and harness](https://github.com/kb5000/trames-runtime/actions/runs/45550128401),
[Linux product ARM64/AMD64/macOS builds](https://github.com/kb5000/trames-linux/actions/runs/45550263256),
[Mac product](https://github.com/kb5000/trames-mac/actions/runs/45550387024), and
[iOS embedding](https://github.com/kb5000/trames-ipad/actions/runs/45550397591).
Hosted build success does not qualify the still-failing KVM service gates or
prove an AMD64 KVM executor exists.
