# Shared runtime application acceptance

The canonical [system design](../../runtime/docs/architecture/acceptance.md) and
manifest/harness live in the shared runtime. The Linux product adapter is
`scripts/validate-applications.py`: native reference plus real KVM SMCCC/MMIO at
one/four workers, with JSON/JUnit/coverage evidence and verified passing reuse.

The dedicated `applications.yml` workflow requires a labelled real KVM runner.
The ordinary build workflow does not claim hardware/software acceptance. The
current [first application baseline](../validation/applications-20261125.md) is
**failing**; known product failures are not xfailed or omitted. The harness and
native-reference CI passing does not turn those failures into compatibility passes.

The runtime's application manifest is architecture-neutral. This product's doctor
is the authority for its currently implemented backend; amd64 build/native jobs
cannot stand in for an amd64 execution adapter. No test machinery is linked into
production or invoked by ordinary program/syscall execution.

Hosted native/harness CI is automatic. The KVM matrix is automated locally; the
GitHub labelled workflow still needs an actual registered runner (repository API
reported zero on 2026-11-25). No permanent runner/listener was installed in this
batch, and no branch/release protection policy was silently changed. Continuous
hardware scheduling and enforced release status remain the next integration step.
