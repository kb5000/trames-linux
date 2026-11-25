# IP, systemd and Docker compatibility — 2026-11-25

Production fixes use shared runtime
`76f860287368605e3697bd934816c18490967c2f`. Linux, Mac and iPad pin its test-only
successor `542b14f9e3dff51d89d61043d5a77916b8417c25`. Product execution changes
belong to ARM64 KVM. Applications,
libraries, inline SVCs and generated instructions remain original.

## Implementation

- Raw/ping ICMP sockets implement partial `ICMP_FILTER` updates, ingress-time
  timestamps and the existing bounded Linux error queue. Both wire ingress
  paths and the local path apply the filter; queued packets retain their arrival
  metadata. Unreachable errors match family, endpoint, echo identifier and bound
  device before entering the error queue. IPv4 and IPv6 options retain their own
  ABI. This is not a claim that every ICMP/PMTU error type is implemented.
- User and user-session keyrings are distinct. LINK/UNLINK/CLEAR/READ operate on
  an owning DAG, with permissions, cycle/depth checks, fork inheritance and
  unlink retirement. READ and DESCRIBE preserve native buffer-query and
  undersized-buffer behavior. The original registry lock orders graph mutation;
  no coordinator or resource service thread is introduced. General key payloads
  and `add_key` are outside this change.
- pidfd fdinfo exposes Pid/NSpid relative to the observing proc mount namespace,
  retains original PID translations and changes to -1 after reap. Accepted
  fdinfo handles retain that capability, without owning the task or its files.
  Both pidfd_open and CLONE_PIDFD bind their original namespace after birth.
- ramfs is a private memory filesystem without tmpfs size limits. debugfs
  exposes only the registered runtime devices, including sync/sw_sync, with
  native mount metadata and namespace/capability rules. Absent debug features
  remain absent. Legacy and new mount APIs share these implementations.
- Overlay layers retain original inode identity across rename, lazy detach,
  removal and pathname replacement. A retired layer is not recreated by a later
  checkpoint, and copy-up revalidates the original inode under the backing lock.
- KVM retains bounded inactive per-mm roots with weak page owners: 32 roots and a 32,768-leaf eviction threshold on address-space switches in the
  16-bit ASID configuration. The active root is not artificially RAM-capped. Retired mm/RAM is
  not pinned by this metadata. Returning to a root revalidates the runtime's
  mapping/storage/permission/code generations.
- Each cached root has a distinct ASID within a worker's disjoint bank. User
  mappings and private EL1 controls are non-global. ASID reuse performs the
  original architectural maintenance before EL0 entry; maintenance targets the
  selected ASID. The hardware's ASID width is queried only when creating a vCPU.
  The 8-bit configuration uses eight roots per worker and a bounded 32-engine
  bank space. Resource exhaustion remains explicit. Mailbox offsets remain
  compile-time assertions; normal SVCs add no KVM_RUN or register-reset ioctl.
- A completed EL1 notification remains pending when a deadline/kick wins the
  userspace return. Its post-SVC ELR must not be returned as an ordinary yield.
  Already handled faults are acknowledged before restoring their saved state.

## Failure isolation

The original service baseline failed generator and containerd startup. Retaining
per-mm roots removed dynamic-loader cold-fault thrashing. Keyring membership,
pidfd fdinfo, ramfs/debugfs and overlay retirement then exposed the real service
lifecycle behavior.

Repeated stock shim startup found an additional intermittent KVM failure. The
same runtime interpreter completed ten original Go initializations; KVM could
fail after a few. Forced cold register restore, pinned host CPU, long time
slices, speculative-range removal, metadata revalidation and per-worker-only
ASIDs did not resolve it. Fault diagnostics observed instruction/exception
inconsistency and damaged parent/child state. Distinct per-root/per-worker ASIDs
with non-global mappings completed 100 consecutive original Go initializations;
the bounded production implementation completed the same 100-run diagnostic.
These version probes are diagnostic evidence, separate from lifecycle acceptance.

The hardware regression directly requires different TTBR0 ASIDs for cached
roots. Removing that binding fails the real KVM assertion with both contexts
zero. Removing completed-notification handling separately fails the pending-SVC
assertion. Positive hardware checks also cover whole GPR/SIMD/TLS/NZCV state
across repeated deadlines, distinct original code/data at identical VAs,
address-space eviction, weak-owner retirement and canonical fork/COW/leases.

## Qualification

The qualification environment is the existing ARM64 Lima/VZ Linux VM, with
nested KVM and real 4096-byte host pages. It is not bare-metal performance data.
CLI remains `078cc9e21ceafd79f631c74bc2f52083d6282a09`. The service image retains
stock Debian 13 systemd 257.13, containerd 1.7.24, Docker 26.1.5 and glibc 2.41.
Native reference workloads run with isolated namespaces and delegated cgroups.

The final tables were independently audited after relocation. Each KVM
column is one transport/worker configuration, not a distinct app.

| Workflow group | Native | SMCCC / 1 | SMCCC / 4 | MMIO / 1 | MMIO / 4 |
| --- | ---: | ---: | ---: | ---: | ---: |
| CLI applications | 12/12 | 12/12 | 12/12 | 12/12 | 12/12 |
| IP address/route/namespace/bridge | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 |
| systemd PID 1 services | 1/1 | 1/1 | 1/1 | 1/1 | 1/1 |
| Docker lifecycle | 1/1 | 1/1 | 1/1 | 1/1 | 1/1 |
| containerd real shim lifecycle | 20 cycles | 20 cycles | 20 cycles | 20 cycles | 20 cycles |

There are **68/68 KVM workflow cells** and 17/17 native reference cells.
Each shim cell includes 20 starts/shutdowns, giving 80 KVM lifecycle repetitions.
Audited tables with adjacent summary JSON/JUnit and raw per-lane cases:
[applications](ip-systemd-docker-20261125/applications/checked-in-report/report.md),
[IP](ip-systemd-docker-20261125/network/checked-in-report/report.md),
[services](ip-systemd-docker-20261125/services/checked-in-report/report.md).

CLI covers shell/coreutils/text/archive/hash, Python files/SQLite and
process/thread/socketpair, local Git, file:// curl, SSH key roundtrip, Node
workers and GCC/Make. It does not claim SSH login or external HTTP/TLS.
IP changes actual addresses/routes and namespaces/bridges. systemd boots as
PID 1, starts one-shot/notify services, reads the journal, stops/restarts services
and checks failed units. Docker uses stock default runc/overlay2, imports an
offline image, creates network/volume, runs/execs containers, checks PID 1 and
seccomp, published HTTP and container DNS, restarts the daemon with persistent
data and cleans up. It does not switch to crun or force reduced GOMAXPROCS.

Additional affected checks, separate from application totals:

| Check | Result | Evidence |
| --- | --- | --- |
| Original file/JIT: host/store, both transports, 1/4 workers | 16/16 | [results](ip-systemd-docker-20261125/file-jit/results.json) |
| Native/KVM keyrings and pidfd/CLONE_PIDFD | 3 native + 12 KVM | [results](ip-systemd-docker-20261125/process/results.json) |
| Shared CLI stdio/env/cwd/PTY/cancellation | 12/12 | [SMCCC](ip-systemd-docker-20261125/cli-smccc/results.json), [MMIO](ip-systemd-docker-20261125/cli-mmio/results.json) |
| Real KVM library/integration | 30/30 | [SMCCC](ip-systemd-docker-20261125/checks/trames-services-asid-smccc-final.log), [MMIO](ip-systemd-docker-20261125/checks/trames-services-asid-mmio-final.log) |
| Same hardware checks, configured 8-bit ASIDs | 30/30 | [SMCCC](ip-systemd-docker-20261125/checks/trames-services-asid-8bit-smccc.log), [MMIO](ip-systemd-docker-20261125/checks/trames-services-asid-8bit-mmio.log) |
| L0 mailbox/permissions/fault/deadline | 10,000 roundtrips per transport | [SMCCC](ip-systemd-docker-20261125/l0/smccc.json), [MMIO](ip-systemd-docker-20261125/l0/mmio.json) |
| Shared affected Linux tests | 1,757 passed; 2 ignored | [log](ip-systemd-docker-20261125/checks/trames-services-linux-tests.log) |
| Shared affected crates/product all-target clippy | passed | [runtime](ip-systemd-docker-20261125/checks/trames-services-linux-clippy.log), [product](ip-systemd-docker-20261125/checks/trames-services-asid-clippy-final.log) |

Configured 8-bit ASIDs ran on 16-bit-capable hardware, not a separate physical
8-bit-ASID machine. Mac workspace and iOS ARM64 FFI compilation passed.
Runtime Ubuntu/macOS [CI](https://github.com/kb5000/trames-runtime/actions/runs/45580339048),
Linux three-platform [CI](https://github.com/kb5000/trames-linux/actions/runs/45584846155)
and Mac [CI](https://github.com/kb5000/trames-mac/actions/runs/45580615776) passed.
The iPad [run](https://github.com/kb5000/trames-ipad/actions/runs/45580629995)
was cancelled during the remaining simulator suite after native checks,
simulator linking and the window-gesture test passed; it is not a full iPad
release acceptance. The final shared acceptance/native-reference
[CI](https://github.com/kb5000/trames-runtime/actions/runs/45591386346) passes on
Ubuntu amd64/arm64 and the Ubuntu/macOS harness after the fork-order correction.
Final test-data/pin-only Linux
[CI](https://github.com/kb5000/trames-linux/actions/runs/45591476306) also passed
all three build/unit platforms (Ubuntu amd64, Ubuntu arm64 and macOS).
The final Mac [CI](https://github.com/kb5000/trames-mac/actions/runs/45591603602)
and iPad [CI](https://github.com/kb5000/trames-ipad/actions/runs/45591591741) are
still running at this update; their results do not replace actual KVM evidence.

## Build identity and reuse

The frozen runner used for all final KVM workflows is clean Linux
1e9f28657f77c47a8caf99117944bcd11baa1a48 / clean runtime
76f860287368605e3697bd934816c18490967c2f. Its SHA256 is
c4b7974ece524e64ba7a226fe89cb762a785d6ea4d168cc545c9f2e2b2f0cb10.
The [receipt](ip-systemd-docker-20261125/build.json) records actual toolchain,
flags and source hashes. Services use the final 7fe39ef manifest; that manifest
is frozen data, not compiled executable code.

Final Linux pin 2abf2157715c06147b67766699e74794d78392ca was built clean separately:
[receipt](ip-systemd-docker-20261125/final-pin-build.json),
[doctor](ip-systemd-docker-20261125/final-pin-doctor.json).
Runtime executable source input SHA256 remains
c04e114af1f6456bf50de811393c3c0a568196983204b3de144f6da8785263f1.
The [ELF comparison](ip-systemd-docker-20261125/pin-equivalence.json) and its
[script](ip-systemd-docker-20261125/checks/compare-service-pin.py) require identical
.text and PT_LOAD headers/bytes after normalizing exactly the embedded runtime
commit, GNU build-ID descriptor and ELF pointer to non-loadable section headers.
Non-loadable symbol/string tables are not execution evidence. Passing runs retain
their original binary provenance; no receipt or reuse key is rewritten.
Broad execution/media tests are not repeated for this test-data/pin-only update.
The two test-only successors change services.json and reorder the CLI fork
reference before creating Python threads. A hosted ARM64 native run exposed a
CPython fork-after-thread-pool DeprecationWarning despite exit 0 and correct
stdout. [Raw failed logs](ip-systemd-docker-20261125/diagnostics-native-python-fork-order/acceptance-native/cases/python.process-threads/stderr)
and their hashes are preserved. No warning is suppressed. The reordered case
passed native and all four KVM configurations; the other 11 workflows were reused
with identical audited evidence.

Unchanged passing service cells reuse audited hashes; the changed RPC case
executes again in every configuration. An initial isolated final-pin build
lacked the guest CLI artifact and issued no receipt; reusing the verified,
unchanged artifact resolved that prerequisite.

Image metadata: [CLI](ip-systemd-docker-20261125/cli-bundle.json),
[services](ip-systemd-docker-20261125/services-bundle.json).
Service tree SHA256 is
1bcfed4955dc5129c417312e1788fe42f41e747ebeda5ee531eba0fd8083fdfd.
Full images/runners remain outside Git in the existing VM/shared artifact
workspace. Checked-in evidence contains no full rootfs or runner.


The [earlier clean build before ASID isolation](ip-systemd-docker-20261125/diagnostics-services-before-asid/report/report.md) passed all four systemd cells,
three Docker cells and one shim-pressure cell. The other shim cells failed or
timed out. Those failures and the unrelated ENOSPC-affected development matrix
are retained as diagnostics, not counted as passes. The corrected 20-iteration shim gate uses real TTRPC Connect/Shutdown RPCs,
validates the reported live shim PID, waits for a zero exit status and bounds
each startup/RPC/reap phase as well as the 600-second case limit. The earlier
plugin-log marker preceded registration of the exit handler; a one-shot SIGTERM
could therefore be ignored by stock containerd. The fixture follows the actual
RPC contract described in [shim startup](https://github.com/containerd/containerd/blob/v1.7.24/runtime/v2/shim/shim.go),
[signal dispatch](https://github.com/containerd/containerd/blob/v1.7.24/runtime/v2/shim/shim_unix.go),
and [the task protocol](https://github.com/containerd/containerd/blob/v1.7.24/api/runtime/task/v2/shim.proto). No failure is xfailed, hidden by a backend fallback or replaced with a
version-only test.

A temporary diagnostic-script operation changed the frozen image's work
directory metadata. The script was removed, its original nanosecond directory
mtime restored from the immutable PAX archive, and the entire image fingerprint
matched again. The active acceptance scratch copy did not contain that script.
The final qualification uses the restored exact tree. Later diagnostics use argv
or stdin rather than editing the image.

## Scope and next gates

Passing these workflows covers their declared operations. It does not establish
all Linux software, all systemd units, all container images or complete cgroup
controller behavior. Docker still truthfully reports unsupported swap/IO limits.
This work does not prove bare-metal native-equivalent performance, AMD64 guest
execution, or physical GPU/audio/video capability. Those remain separate product
and hardware gates. The dispatchable hardware jobs still require a registered
KVM-capable runner; ordinary hosted builds do not count as hardware acceptance.
