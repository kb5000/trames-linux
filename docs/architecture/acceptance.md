# Shared runtime application acceptance

The canonical [system design](../../runtime/docs/architecture/acceptance.md) and
manifest/harness live in the shared runtime. The Linux product adapter is
`scripts/validate-applications.py`: native reference plus real KVM SMCCC/MMIO at
one/four workers, with JSON/JUnit/coverage evidence and verified passing reuse.

The dedicated `applications.yml` workflow requires a labelled real KVM runner.
The ordinary build workflow does not claim hardware/software acceptance. The
historical [first application baseline](../validation/applications-20261125.md)
failed. The [fix and expanded acceptance record](../validation/applications-services-20261125.md)
now records 48/48 basic application cells passing, while the additional network
and system-service gates retain their observed failures. Harness/native CI
passing does not turn a failed KVM workflow into a compatibility pass.

The runtime's application manifest is architecture-neutral. This product's doctor
is the authority for its currently implemented backend; amd64 build/native jobs
cannot stand in for an amd64 execution adapter. No test machinery is linked into
production or invoked by ordinary program/syscall execution.

Hosted native/harness CI is automatic. The KVM matrix is automated locally; the
GitHub labelled workflow still needs an actual registered runner (repository API
reported zero on 2026-11-25). No permanent runner/listener was installed in this
batch, and no branch/release protection policy was silently changed. Continuous
hardware scheduling and enforced release status remain the next integration step.

## System services and persistent stores

`network.json` and `services.json` add real IP configuration/data-plane and
stock systemd/Docker lifecycle gates. `services.yml` accepts a native-architecture
Debian rootfs URL plus SHA256, freezes the prepared image, then runs native and
KVM SMCCC/MMIO at one/four workers. It uses the same strict shared evidence
pipeline. A registered hardware runner is still required for GitHub dispatch.

```sh
sudo python3 scripts/prepare-services.py --source rootfs.tar.gz --sha256 SHA256 --output target/services-bundle
sudo python3 scripts/validate-applications.py --manifest runtime/tests/acceptance/services.json --filesystem store --network outbound --native-container /usr/bin/systemd-nspawn --bundle target/services-bundle --runner target/release/trames --receipt target/acceptance-build.json --output target/services-matrix
```

`trames run --store DIR --rootfs DIR_OR_ARCHIVE -- /absolute/guest/program`
creates a new private VFS. `--store DIR` alone reopens it; a stopped file can be
exported using `trames read-file --store DIR -- /guest/path`. Import is create-only,
uses the existing single-writer lease and unpublished staged-population API, and
never replaces an existing directory. An interrupted import cannot be executed.
The outer directory contains provenance and an import marker, while `fs/` retains
the shared store format. Ordinary `--rootfs DIR` keeps host directory semantics;
proc/sys/devpts and guest devices are attached before entering that root.

The controller freezes the binary, receipt and harness once per matrix, so
parallel builds cannot switch a running matrix onto another binary. No acceptance
checks, test polling, or extra service threads enter production syscall paths.

Use `--network off|loopback|outbound|full` to opt into the existing per-runtime
virtual fabric and its host-access policy. Without this option the original
disabled policy is retained. Network/services profiles select `outbound`;
published Docker ports in these tests are exercised inside the guest network,
not exposed as host services. The product now separately supports named networks
and authorized host publication; see [usage](../guides/environments.md) and the
[372-operation KVM matrix](../validation/networks-20261127.md). The original managed Docker bootstrap gap is now closed by the
[private-store runc and host-service acceptance](../validation/runc-managed-20261127.md),
with original containers, daemon restart and actual host HTTP.
