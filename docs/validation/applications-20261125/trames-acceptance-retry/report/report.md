# Runtime application acceptance

Each cell is one workflow in one execution configuration. Missing/blocked/reference failures do not count as passes.

| Workflow | native | kvm-smccc-1 |
| --- | --- | --- |
| archive.roundtrip | passed (reused) | failed |
| build.gcc-make | passed (reused) | failed |
| coreutils.files | passed (reused) | failed |
| curl.file-transfer | passed (reused) | failed |
| git.local-workflow | passed (reused) | failed |
| hash.base64 | passed (reused) | failed |
| node.workers | passed (reused) | failed |
| python.process-threads | passed (reused) | passed (reused) |
| python.stdlib | passed (reused) | failed |
| shell.pipeline | passed (reused) | failed |
| ssh.key-roundtrip | passed (reused) | failed |
| text.filters | passed (reused) | failed |

Timing includes setup/startup and is not a qualified performance comparison.
