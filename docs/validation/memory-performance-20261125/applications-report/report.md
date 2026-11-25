# Runtime application acceptance

Each cell is one workflow in one execution configuration. Missing/blocked/reference failures do not count as passes.

| Workflow | native | kvm-smccc-1 | kvm-smccc-4 | kvm-mmio-1 | kvm-mmio-4 |
| --- | --- | --- | --- | --- | --- |
| archive.roundtrip | passed (reused) | passed | passed | passed | passed |
| build.gcc-make | passed (reused) | passed | passed | passed | passed |
| coreutils.files | passed (reused) | passed | passed | passed | passed |
| curl.file-transfer | passed (reused) | passed | passed | passed | passed |
| git.local-workflow | passed (reused) | passed | passed | passed | passed |
| hash.base64 | passed (reused) | passed | passed | passed | passed |
| node.workers | passed (reused) | passed | passed | passed | passed |
| python.process-threads | passed | passed | passed | passed | passed |
| python.stdlib | passed (reused) | passed | passed | passed | passed |
| shell.pipeline | passed (reused) | passed | passed | passed | passed |
| ssh.key-roundtrip | passed (reused) | passed | passed | passed | passed |
| text.filters | passed (reused) | passed | passed | passed | passed |

Timing includes setup/startup and is not a qualified performance comparison.
