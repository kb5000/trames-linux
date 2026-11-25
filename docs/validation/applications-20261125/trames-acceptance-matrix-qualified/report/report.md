# Runtime application acceptance

Each cell is one workflow in one execution configuration. Missing/blocked/reference failures do not count as passes.

| Workflow | native | kvm-smccc-1 | kvm-smccc-4 | kvm-mmio-1 | kvm-mmio-4 |
| --- | --- | --- | --- | --- | --- |
| archive.roundtrip | passed | failed | failed | failed | failed |
| build.gcc-make | passed | failed | failed | failed | failed |
| coreutils.files | passed | failed | failed | failed | passed |
| curl.file-transfer | passed | failed | passed | failed | failed |
| git.local-workflow | passed | failed | failed | failed | failed |
| hash.base64 | passed | failed | failed | failed | failed |
| node.workers | passed | failed | failed | failed | failed |
| python.process-threads | passed | passed | passed | passed | passed |
| python.stdlib | passed | failed | failed | failed | failed |
| shell.pipeline | passed | failed | failed | failed | failed |
| ssh.key-roundtrip | passed | failed | failed | failed | failed |
| text.filters | passed | failed | failed | failed | passed |

Timing includes setup/startup and is not a qualified performance comparison.
