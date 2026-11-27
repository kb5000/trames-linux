# Managed runc, private stores and host services — 2026-11-27

The named-environment Docker failure is fixed in shared Runtime commit
`5cc2476bea28f44af095752811c026868f1e442b`. It is preserved in integrated Runtime
`bb2748532f7ea56d658d819f8afdfd114b7a01df`, alongside the independently developed
memory-lifecycle work. Linux pins the isolated tested commit.

## Cause and change

Original runc first copies its executable into a sealed memfd and reexecutes it.
Its next read of `/proc/self/exe` returned EIO: the shared pathname opener did not
recognize the retained executable inode as sufficient authority after CLOEXEC
closed the last numeric descriptor. It requested owner fallback without a
prepared open operation. The ordinary private-store syscall path then returned
EIO. The host-backed standalone path had avoided this classification.

The opener now accepts the original executable inode capability, just as it
already accepts an original proc-fd inode. It uses normal open/permission/seal
handling and creates a fresh open file description. The executable's actual
lifetime keeps the inode alive. This supports ordinary application self-reexec;
Trames does not patch executable instructions.

Once bootstrap succeeded, original runc exposed the same classification problem
for O_PATH handles on freshly mounted devpts. These handles retain inode/mount
identity without opening a device or rendering volatile bytes. They now use the
shared open path. Regression checks preserve inode identity and require that an
O_PATH ptmx open allocates no PTY. Actual device opens keep their existing
permission, task-transition and capacity handling.

The upstream [self-clone code](https://github.com/opencontainers/runc/blob/v1.1.15/libcontainer/nsenter/cloned_binary.c)
and [bootstrap code](https://github.com/opencontainers/runc/blob/v1.1.15/libcontainer/nsenter/nsexec.c)
explain the original application's sequence. No application/library replacement,
crun fallback, forced GOMAXPROCS, coordinator, service thread or CPU-loop check
was added.

## Acceptance

[Summary](runc-managed-20261127/summary.json) and
[final source-bound build receipt](runc-managed-20261127/final-source-build.json).
The final runner SHA256 is
`8d6c28bfd6d5d9886d3e6ffb3c23a62323cfabe66b4f9fee5cca7b8d26ff0078`.
The shell-only harness correction rebuilt identical runner bytes; original
per-run receipts remain alongside the final source receipt.

| Scope | Result |
| --- | --- |
| Original systemd/Docker, SMCCC/MMIO × one/four CPUs | Four cohorts, 64 operations passed |
| Original shell-managed Docker, SMCCC/MMIO × four CPUs | Two cohorts, 32 operations passed |
| Original sealed memfd self-reexec, SMCCC/MMIO × one/four CPUs | 16 reexecs, 32 management operations passed |
| Native Linux self-reexec reference | Passed before KVM, same original C ELF |
| All-feature VFS library, Linux / Apple | 1186 / 1178 passed, one existing ignored case each |
| Original sealed self-reexec on real Mac HVF, one/four CPUs | Six passed; [paired record](https://github.com/project-trames/trames-mac/blob/main/docs/validation/runc-fix-20261127.md) |
| Strict all-target/all-feature VFS Clippy | Passed on Linux and Apple |

All six Docker cohorts use stock default runc/overlay2 and the existing offline
BusyBox image. They verify container PID1 and seccomp, persistent volume bytes,
exec, container-name DNS, and actual host HTTP through named-network publication.
A stopped container no longer serves host HTTP. Restarting the daemon retains a
stopped container and its data; starting it restores actual host HTTP. Final
cleanup and environment stop release the actual native host port.

`scripts/validate-memfd-exec.py` builds an independent Linux C program which copies
its own ELF into a sealed memfd, reexecutes it and reopens the original image.
The native reference passes. The exact frozen pre-fix runner
`c65e891b5021cb36025568ead1b064b5ee2d2aafaf96e89b174ea80a889e57c7`
reproduces EIO, recorded separately from accepted passes. Repeated exec and
restart pass on the fixed runner. Native reference checks run in Linux CI; the
existing KVM workflow runs the complete executable matrix. Full offline service
checks remain an explicit local command against an existing image.

The earlier [network record](networks-20261127.md) retains its original failed
Docker evidence. This record closes that gap rather than relabeling its failed
runs. Early candidates also exposed the devpts failure and two previously
unreachable harness mistakes (a Python module-name collision and a shell restart
separator); acceptance above requires the complete corrected workflows.

CPU/4 KiB/graphics implementations are unchanged. Their prior hardware results
are reused; the exact original application workflow and affected VFS/platform
paths were validated for this change.

## CI closure

[Recorded jobs/steps](runc-managed-20261127/ci.json): shared Runtime
[Linux/macOS full CI](https://github.com/project-trames/trames-runtime/actions/runs/45956151742)
and Linux product [AMD64/ARM64/macOS build CI](https://github.com/project-trames/trames-linux/actions/runs/45956284120)
passed, including the new native sealed-executable reference. Runtime's initial
formatting failure in the independently integrated memory lifecycle was closed
with formatting only; the tested VFS feature commit is preserved.

The initial [presentation run](https://github.com/project-trames/trames-linux/actions/runs/45956284245)
passed AMD64 and ARM64 WSI/desktop/interop/keyboard, then ARM64's original native
Wayland reference missed a gesture event. [Targeted recovery](https://github.com/project-trames/trames-linux/actions/runs/45957270869)
passed both Wayland window cohorts and parent protocol cases, plus all three
Wayland HiDPI cohorts; its native X11 pixel check then failed. The final
[X11-only recovery](https://github.com/project-trames/trames-linux/actions/runs/45958385557)
passed the failed software and previously unrun Vulkan X11 cohorts.
[Original failures](runc-managed-20261127/ci-native-reference-initial.log),
[original pixel failure](runc-managed-20261127/ci-hidpi-initial.log) and
[final recovery output](runc-managed-20261127/ci-hidpi-recovery.log) are retained.

These references used identical product/Runtime sources. No graphics code,
assertion or timeout was changed; passed cohorts and platforms were reused.
A successful retry does not establish the intermittent reference failures'
root cause or claim them fixed. Temporary refs are deleted after integration.

The paired [final Mac CI](https://github.com/project-trames/trames-mac/actions/runs/45957042856) also passed all-feature checks/tests, native integrations, and Dev/Stable builds. Its compiler-only HVF recursion setting matches the shared memory crates. Both owning platform paths are now closed.
