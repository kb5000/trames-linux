# Linux named environments — 2026-11-27

The Linux product now creates and manages persistent named Linux environments.
Human names and UUIDs select the shared manifest/private-store layout. Background
workers expose the same Runtime exec and terminal protocols used by Mac, while
the independent CLI repository retains clients and argument/terminal policy.
See the [usage guide](../guides/environments.md).

Creation supports pinned official Alpine/Debian archives and original custom
rootfs directories/archives. Configuration uses validated atomic writes, protects
registered identity/provenance, and supports global defaults, per-environment
profiles, resource limits, sysctl policy and explicit host directory grants.
Lifecycle includes start/stop/restart, independent exec and detachable terminals,
status/logs, rename/remove, clone, import-store and portable backup/restore.
New identities never inherit the source's host directory authority.

One environment worker owns its store and drives existing Runtime workers,
HostWait/HostWake and pipes/PTYs. Startup uses an inherited readiness stream;
stop uses a private socket rather than PID-file signals. Stop acknowledges after
joining execution and syncing storage. No new coordinator, service thread or
polling bus was introduced. Protocol constants and platform boundaries use
compile-time checks; profile validation happens on cold management/start paths.

## Recorded acceptance

[Summary](environments-20261127/summary.json) and
[source-bound build receipt](environments-20261127/build-receipt.json) identify
exact inputs, the actual ARM64 Linux compiler and final runner bytes.

| Scope | Result |
| --- | --- |
| Real ARM64 KVM, SMCCC/MMIO × one/four CPUs | [282 operations passed](environments-20261127/kvm.json) |
| Original systemd PID1 and a real service ExecStop marker | Graceful stop passed in all four KVM cohorts |
| Named X11/Wayland, clipboard allowed/denied | [8 cases passed](environments-20261127/managed-desktop.json), same final runner |
| Official Alpine and Debian, create/exec/sync/stop/reopen | [12 operations passed](environments-20261127/builtins.json), including pinned-checksum rejection |
| Existing standalone CLI, PTY, signals and deadlines | [SMCCC 10/10](environments-20261127/standalone-smccc.json), [MMIO 10/10](environments-20261127/standalone-mmio.json) |
| Apple build without a KVM execution claim | [39 cold operations passed](environments-20261127/cold-apple.json) |
| Product Linux app/CLI tests | 11 app and 3 CLI tests passed; 4 native UI cases remain explicitly ignored in ordinary units |
| Shared distro and host server tests | 44 + 5 passed on Linux and Apple; final provenance regression also passed on Apple |
| Scoped locked Clippy, formatting, pins and license fingerprints | Passed on Linux and Apple |

The matrix covers transactional configuration failure, aliases/default selection,
read-only shares, backup CRC/path/inventory rejection, independent UUIDs, busy
owner exclusion, separate binary streams and exit codes, literal argv/env/cwd,
parallel exec, independent environments, detach/reattach, sync/reopen, SIGKILL
recovery, restart and actual systemd shutdown. The original shared guest binaries
and libraries execute without instruction rewriting.

The final runner SHA256 is
`1c023b788bc3bbfb04cffaad162e000739744ebd376b1b1090f85d677a18c314`.
Linux Runtime pin: `46b6bc40b3561a918154653a23bc13c8883503ee`; integrated Runtime
main: `3f5d4f53bb3dae0449b03cd0a8a0fe1ea8562cc8`. Both preserve the tested feature
commits. CLI pin remains `8dd30e46dffdaf4fd9c412591edb557748e8d737`.
Mac's final paired adaptation separately passed its 13 real-HVF groups.

## Findings resolved during acceptance

- Normal managed shell cancellation now returns a successful stop after sync.
- Newly accepted control peers wake the original wait so the descriptor set is
  rearmed before sleeping. Retirement preserves replacement socket paths.
- Original systemd needs `container=trames` and `/run/host/notify`, matching the
  established Mac boot path. Shutdown is verified by guest ExecStop output;
  a 30-second forced cancellation is an error, not graceful-stop success.
- UUID-based rename updates a name-based default selection too. Profile edits
  cannot change registered image provenance. Internal readiness descriptors are
  validated before ownership is taken.
- Socket unit fixtures initially used the VM's shared TMPDIR, which cannot host
  Unix sockets. Both passed after using native `/tmp`, as the product does.
- Runtime CI initially rejected the new workspace member. Its boundary inventory
  now includes the host crate and checks its independence from CLI/CPU/products.

Ordinary build CI runs the cold matrix on Linux AMD64, Linux ARM64 and Apple;
that is not hardware qualification. The required KVM workflow runs the real
named-environment matrix. The portable harness accepts `--systemd-rootfs` for an
existing systemd image. Local systemd cases ran as root only to read the existing
root-owned reference image; ordinary user named desktop/clipboard and CLI paths
were tested separately. All registries were disposable test directories.

This scope does not qualify a physical GPU, native-level performance, arbitrary
Linux software, power-loss durability, or a full release. Existing HiDPI/CPU/
network results are reused where their code is unchanged. Login autostart, Linux
GUI management, named bridge publishing, managed Android and additional native
interop services remain future work; unsupported required backends are rejected.
Test binaries and rootfs payloads are not committed.
