# Runtime application acceptance

Each cell is one workflow in one execution configuration. Missing/blocked/reference failures do not count as passes.

| Workflow | native | kvm-smccc-1 | kvm-smccc-4 | kvm-mmio-1 | kvm-mmio-4 |
| --- | --- | --- | --- | --- | --- |
| archive.roundtrip | passed (reused) | passed (reused) | passed (reused) | passed (reused) | passed (reused) |
| build.gcc-make | passed (reused) | passed (reused) | passed (reused) | passed (reused) | passed (reused) |
| coreutils.files | passed (reused) | passed (reused) | passed (reused) | passed (reused) | passed (reused) |
| curl.file-transfer | passed (reused) | passed (reused) | passed (reused) | passed (reused) | passed (reused) |
| git.local-workflow | passed (reused) | passed (reused) | passed (reused) | passed (reused) | passed (reused) |
| hash.base64 | passed (reused) | passed (reused) | passed (reused) | passed (reused) | passed (reused) |
| node.workers | passed (reused) | passed (reused) | passed (reused) | passed (reused) | passed (reused) |
| python.process-threads | passed | passed | passed | passed | passed |
| python.stdlib | passed (reused) | passed (reused) | passed (reused) | passed (reused) | passed (reused) |
| shell.pipeline | passed (reused) | passed (reused) | passed (reused) | passed (reused) | passed (reused) |
| ssh.key-roundtrip | passed (reused) | passed (reused) | passed (reused) | passed (reused) | passed (reused) |
| text.filters | passed (reused) | passed (reused) | passed (reused) | passed (reused) | passed (reused) |

Timing includes setup/startup and is not a qualified performance comparison.
