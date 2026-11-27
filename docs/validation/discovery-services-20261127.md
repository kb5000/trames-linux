# Environment DNS and native user services — 2026-11-27

Linux adds human environment-name DNS through shared Runtime
`8bd058556ffb35be8e127afbaf7c1c26dae998d9`, and native systemd user-service
installation, login enablement, readiness, stop, failure restart and CLI lifecycle
integration. No guest instruction rewriting, full guest Linux kernel, new network
service thread or CPU hot-path check is introduced.

## Accepted execution

The final ARM64 KVM runner `c732aec5969523d96d11ad8b2926c148f9779650213e24339f4977516be0158c` passed **628 operations**, SMCCC/MMIO ×
one/four CPUs, including all four original guest systemd stop checks.
[Result](discovery-services-20261127/results.json),
[commands](discovery-services-20261127/operations.json) and the frozen build
receipt/harness/input hashes are retained alongside each cohort's result and
native user journal. Native units use test-owned transport drop-ins; the harness
verifies the actual worker environment, not just the cohort's label.

Each cohort covers original getaddrinfo A/AAAA and curl HTTP by environment name;
qualified/case-folded/legacy numeric aliases; separate-network rejection; original
host HTTP publication; same-name startup rejection; SIGKILL with automatic native
systemd recovery and retained guest data; stale crash-name invalidation; stop,
rename, restart; clone and archive restore with fresh UUIDs/no inherited host
unit; and enable/disable with and without immediate process handoff.

The real native unit's MainPID equals the Unix command socket's kernel peer PID.
It is verified by systemd-analyze, installed under the user's default target,
and enabled through its actual wants link. No user target is restarted and no
logout/login is forced: **fresh login itself is not measured**. This is user
login enablement, not a claim of unattended machine-boot service availability.

For original guest systemd, a real oneshot service writes an ExecStop marker.
Native user-service stop returns a clean guest exit, then managed reopening in
shell mode reads that persisted marker. Thus a host cancellation alone cannot
satisfy the stop assertion.

Actual Mac HVF also passed one/four CPUs, **66 commands** on the paired Runtime;
[Mac record](https://github.com/project-trames/trames-mac/blob/main/docs/validation/environment-dns-20261127.md).
Local Linux/Apple shared checks passed: distro 45, network 213, virtual integration
15 (Apple reuses the 14 unchanged passing integrations and reruns the corrected
new DNS test). Linux and Apple product unit/CLI checks and strict affected Clippy
passed. Runtime/Mac full CI and Linux product CI are tracked in the closure ledger.

## Ownership and lifecycle

The shared registry supplies the registered name only at startup. Endpoint-owned
file leases publish the name and address; DNS consults a bounded record and checks
the matching socket identity only for a local DNS question. Crash closes the
leases even if a pathname remains. Normal packets and CPU execution do not scan
records or poll a new bus. Case-insensitive collisions fail startup. TTL zero and
`NAME.env.trames` preserve lifecycle correctness and reserved numeric/gateway aliases.

Linux's native unit executes the existing worker directly with Type=notify.
READY means exec sockets/workers are available; guest services keep their own
readiness. ExecStop uses the existing guest control protocol. Automatic failure
restarts retain systemd's rate limit; explicit start clears failed-unit counters.
Steady-state exec reuses its ready owner without spawning systemctl. UUID-bound
units survive stopped-environment rename; clones/restores do not copy a host unit.

[User commands and limits](../guides/environments.md#login-autostart-and-native-user-services)
cover stable executable paths and host user-manager lifetime. Unit configuration
is serialized per object; its lifecycle lock is released before native ExecStop.
All systemctl/journal work is in the host product's cold management path.

## Diagnostic attempts and fixture provenance

The first candidate hit native systemd's rapid-start limit after several deliberate
restarts. Explicit CLI start/restart now clears the failure budget, while already
running exec does not. A later harness attempt used the explicit `--store` reader
on a managed store, whose import marker differs; the final harness reopens through
the owning managed path. These failed attempts and their input receipts remain in
`diagnostics/`; neither is counted as an accepted pass. An initial new DNS TCP test
was corrected to reuse its persistent connection rather than exhaust the existing
bounded server pool; original 14 integration passes were retained.

The existing offline Debian fixture was reviewed before use. No private host keys,
SSH/user credentials or unlockable accounts were present. A task-owned archive
excludes user content, authentication files, SSH, machine identities, logs, caches
and the unrelated Docker payload. Input SHA256 and audit are retained; the original
fixture was not changed, and no new image was downloaded. Guest identity is created
by the existing managed environment path. No rootfs/binary archive is checked in.

## CI closure and separate native reference failure

The final [Linux three-platform build/unit CI](https://github.com/project-trames/trames-linux/actions/runs/45968859481), [Runtime Linux/macOS CI](https://github.com/project-trames/trames-runtime/actions/runs/45966807752), and [Mac Dev/Stable CI](https://github.com/project-trames/trames-mac/actions/runs/45967467914) passed.
The first dependency-record check found an outdated lockfile fingerprint;
regenerating the reviewed inputs corrected it, and [license CI](https://github.com/project-trames/trames-linux/actions/runs/45968859602) passed. No new third-party coordinates were introduced.
The [source closure](discovery-services-20261127/source-closure.json) and clean
closing build receipt prove the final runner is byte-identical to the accepted
628-operation runner. Documentation and a CI-trigger comment reuse those results.

The unrelated [native presentation run](https://github.com/project-trames/trames-linux/actions/runs/45968465203) passed AMD64 but ARM64 stopped in the standalone Wayland reference before reaching Trames Wayland execution: gesture v was absent.
The [scoped window recovery](https://github.com/project-trames/trames-linux/actions/runs/45970309953) reproduced it. It is **not counted as passed**; subsequent ARM64 parent/HiDPI stages were not reached in that retry.
Both failures, their tested commit identities and exact failed/skipped stages
are retained in the [CI ledger](discovery-services-20261127/ci.json).
No graphics assertions, deadlines or production display code were changed.

Follow-up: reproduce pointer focus/enter/button delivery in the original
Weston/Xvfb reference and repair its input synchronization before repeating only
the failed and unfinished cohorts. This remains separate from this delivery's
DNS/native-service acceptance and the owner's deferred Linux GUI work.

The native-user-manager audit confirms all 35 test unit identities have no
remaining unit, drop-in, enablement link or MainPID. The audited temporary image
and duplicate input trees were removed, reclaiming 193,357,961 bytes. The original
offline fixture and existing shared build caches remain intact.
