# Named environments on Linux

The ARM64 Linux product can manage multiple persistent Linux environments using
the same manifest, UUID layout and private VFS store as the Mac product. KVM
executes the original application instructions; the shared userspace Runtime
continues to own Linux process and resource semantics.

```sh
trames env create work --distro alpine
trames env ls
trames -e work shell
# Ctrl+] detaches without stopping the shell.
trames -e work session ls
trames -e work session attach 1
trames -e work exec -- sh -c 'uname -m; echo hello'
trames env stop work
```

`env create` is the only command that downloads an image. Built-in Alpine and
Debian archives must match the Runtime's pinned SHA256. Use `--archive FILE` for
an already downloaded built-in image, or `--rootfs DIR_OR_ARCHIVE` for an original
custom rootfs. Custom archives may specify `--sha256 HASH`. Directories are
imported into a new store; the source is not used as writable guest storage.
A required shell or `/sbin/init` must exist before creation is published.

For a CPU-only host without a native Vulkan provider, select
`trames config --global set defaults.components.vulkan false` before creating
environments, or set `components.vulkan false` on an existing environment.

The default registry is `$XDG_DATA_HOME/trames` or `$HOME/.local/share/trames`.
`TRAMES_HOME` selects an absolute alternate registry. Every environment has a
stable UUID directory, profile, host grants and `store/`; names and UUIDs both
select it. Existing explicit `run --rootfs` / `--store` commands keep their
original standalone storage format. Import those stores with
`env import-store NAME DIRECTORY`.

```sh
trames config --global set default_environment work
trames -e work config set resources.cpus 4 resources.memory 2GiB
trames -e work config set kernel.sysctl '{"kernel.threads-max":4096}'
trames -e work config show --json
trames -e work config validate
trames env share work sources /absolute/host/project /work ro
trames env restart work
```

Global `defaults.*` configure future environments. Without an explicit host
profile, boot rereads the guest `/etc/trames.toml` against those defaults. A host
`config set` or `--config FILE` creates an explicit profile. Configuration edits
are validated as a complete transaction and take effect on the next start;
invalid edits leave the previous file intact. Host grant files remain bound to
the environment UUID. A guest request cannot grant a new host directory.
`env share` records the selected host directory and a read-only or writable
mount. Mounts honor the shared manifest capability and host grant intersection.

`env start` starts a background worker and waits for the loaded execution session
and its sockets. `exec` and `shell` start an existing stopped environment on
demand. They never create or download one. Commands preserve literal arguments,
separate binary stdout/stderr, environment overrides, working directory and exit
status. `exec -t` uses a private guest PTY. Each `shell` opens an independent
terminal, retained across detach/attach. Startup readiness does not mean that all
PID1 services have reached their application-specific ready state.

One worker owns each running store. Existing descriptor readiness drives exec,
console and stop control on the original foreground wait. No extra coordinator,
service thread or polling bus is added. `env stop` waits for Runtime exit and
store synchronization before acknowledging and releasing ownership. Systemd
receives its ordinary shutdown signal; a 30-second fallback cancels and syncs
with an error. An abruptly killed worker releases its kernel file locks; the
next start replaces stale sockets. Explicit guest `sync` establishes the data
boundary used by crash-recovery acceptance.

```sh
trames env clone work scratch
trames env backup work /absolute/path/work.tar.gz
trames env restore recovered /absolute/path/work.tar.gz
trames env rename scratch experiment
trames env remove experiment --yes
trames env inspect work --json
trames env logs work
```

Clone, backup and remove require a stopped, unowned environment. Backups include
an inventory of original store bytes and metadata. Restore rejects invalid
paths, nonregular entries, duplicates, checksum mismatch and damaged gzip
trailers. Clone/restore/import assign a new UUID and machine identity, clear
mount requests, and use the recipient registry's default grants. Select shared
directories again. Backups do not include host grant files. Removing an
environment deletes its private store, not selected host directories.

Named environments use existing native graphics and audio adapters when enabled.
A desktop is attached when windows, host UI request, host grant and a host X11 or
Wayland display are available. Clipboard exchange additionally requires both
clipboard request and grant. Headless environments do not start a presenter.
`boot.mode=systemd` requires an image with original systemd installed. Alpine's
built-in image does not supply it.

Named networks use the shared Runtime IP fabric and existing descriptor waits.
They do not configure host bridges, TAP devices, namespaces or firewall rules.
`host` mode shares the actual host socket space; `none` disables networking.
A bridge isolates guest loopback and port bindings while connecting its members:

```sh
trames network create development
trames network create private --internal
trames -e work config set network.mode bridge network.bridge development
trames env publish work 127.0.0.1:8080 80
trames env publish work 127.0.0.1:5353 53 udp
trames env restart work
trames network ls --json
trames network inspect development
# Desired configuration changes apply on the next start/restart.
trames env unpublish work 127.0.0.1:8080
trames env grant work ports off
trames env stop work
trames network remove development
```

Publications require both `interop.ports.enabled` and the host `ports` grant.
A bare host port defaults to `127.0.0.1`. Explicit `0.0.0.0:PORT` publishes on
all IPv4 host interfaces; IPv6 addresses use `[::1]:PORT` syntax. Ordinary host
bind permissions and port conflicts apply. A failed startup releases all ports
already opened by that startup. Stop releases them after teardown.

Each network assigns registered environment UUIDs a stable ID from 2 through
254, preserved across restart and rename. Its IPv4 address is `10.0.0.ID`, IPv6
is `fd00::ID` (ID in hexadecimal), and the shared resolver serves `envID.trames`.
Different networks have independent address spaces. Within one network, a running
`db` environment resolves as `db.trames` and `db.env.trames` (A and AAAA). The
qualified `NAME.env.trames` form also covers names reserved by `gateway.trames`
and the existing `envID.trames` aliases. Names are case insensitive; starting a
second environment with the same folded name in that network fails instead of
routing to an ambiguous peer. The registry's existing name syntax is retained.

Names follow live endpoint leases. Stop or process death invalidates them even
if socket files remain. Local answers use TTL zero. Rename requires stopping the
environment; its next start retains its UUID/address and publishes its new name.
Clones/restores receive new UUIDs and names. Deleted UUID allocations are reclaimed
on the next member start. These names are served inside Trames networks; host
clients use published host ports. Inspect the member map to find addresses. Networks must have all
attached environments stopped before removal; update their profiles before the
next start. Default networks allow host-backed outbound NAT; `--internal`
blocks external IP traffic while retaining peer and gateway/resolver services.
Host publication remains a separately authorized inbound capability.

This iteration does not add a Linux GUI manager, managed Android, VA-API,
interpreter thunking, or guest host-exec/file-control services. Host/none networking and existing native
compute/display/audio paths remain available. Manifests are portable data;
actual host capabilities and execution backends are platform-specific.

Automation: `scripts/validate-environments.py --native-only` checks cold registry
and storage operations on all CI hosts. With a source-bound `--receipt` and real
KVM it also checks both transports, one/four CPUs, exec/PTY, isolation,
persistence and crash recovery. `--systemd-rootfs DIR` adds original PID1 service
startup and graceful shutdown verification using an existing image.

`scripts/validate-networks.py --native-only` checks cold network configuration.
With a source-bound receipt it checks real KVM, both transports and one/four
CPUs, original IPv4/IPv6 TCP/UDP, isolation/NAT, actual host ports, rollback and
restart. `scripts/validate-network-services.py --rootfs DIR` additionally probes
an existing offline systemd/Docker image through real host HTTP, container DNS,
exec, seccomp, persistent volumes and daemon restart. Use `--cpus 1` or `4`;
`--init shell` also checks shell-managed daemons.
`scripts/validate-memfd-exec.py` checks original sealed executable self-reexec
on native Linux and both KVM transports.

## Login autostart and native user services

On a Linux host with a systemd user manager:

```sh
trames env stop db
trames env enable db --now
trames env inspect db --json
trames env logs db
trames env restart db
trames env disable db --now
```

`enable` installs a per-user unit and enables it under `default.target`; `--now`
also starts it. Without `--now`, a running ordinary environment continues under
its existing owner until stopped. `disable` removes login enablement and the
managed unit; `--now` also stops the environment. No root/system unit or linger
setting is changed. Login and logout lifetime follows the host's user-manager
policy; this is not an unattended machine-boot promise.

The unit runs the existing environment worker in the foreground with
[`Type=notify`](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html).
Readiness means command sockets and execution workers are available; services
inside the guest retain their own readiness checks. Native stop uses the existing
guest shutdown protocol, allowing guest systemd to execute service `ExecStop`
actions before storage teardown. Failure restarts are bounded; explicit CLI
start/restart resets a failed unit's start-limit counter. Already-running exec
requests reuse their owner without invoking systemctl.

Units bind the environment UUID and registry, so stopped-environment rename
preserves ownership. Clones and restores do not inherit a host unit. Disable a
managed unit before removing its environment. `env inspect --json` includes its
unit/path/install/enablement information; `env logs` reads that unit's user journal.
The unit records the current executable's absolute path, so install Trames at a
stable path before enabling environments; re-enable after moving it. Paths are
quoted for systemd directly, without a shell.

`scripts/validate-discovery-services.py --runner BINARY --receipt BUILD.json
--output DIR [--systemd-rootfs EXISTING_ROOTFS_OR_ARCHIVE]` requires real KVM and
an existing user manager. It covers both transports and one/four CPUs, original
curl HTTP by environment name, IPv6 lookup, isolation, crash recovery, rename,
clone/restore, native publication and real guest PID1 shutdown when supplied.
It uses only disposable registries/UUID-named user units and does not restart the
user's login target. Fresh logout/login itself is not part of this acceptance.
