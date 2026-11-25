# Paired original workload baseline

Audit: **passed**.
108 measured executions; calibration/warmup excluded.

Medians are milliseconds. Ratio is KVM/native work time (lower is better).
Wall and CPU include startup; sampled memory may miss short peaks.
This host/configuration only; caches retained; no bare-metal claim.

| Workload | Workers | Lane | Work ms | Wall ms | CPU ms | Work/native |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| build | 1 | mmio | 26002.491 | 26079.593 | 25909.296 | 6.276 |
| build | 1 | native | 4143.382 | 4145.223 | 4054.375 | 1.000 |
| build | 1 | smccc | 23850.238 | 23931.838 | 23757.281 | 5.756 |
| build | 4 | mmio | 43214.924 | 43331.898 | 148298.873 | 36.396 |
| build | 4 | native | 1187.348 | 1188.612 | 4689.751 | 1.000 |
| build | 4 | smccc | 39314.008 | 39418.770 | 151047.865 | 33.111 |
| cpu | 1 | mmio | 57.527 | 73.454 | 72.812 | 1.057 |
| cpu | 1 | native | 54.410 | 54.865 | 54.566 | 1.000 |
| cpu | 1 | smccc | 57.323 | 71.720 | 71.193 | 1.054 |
| cpu | 4 | mmio | 88.446 | 128.215 | 272.419 | 1.537 |
| cpu | 4 | native | 57.549 | 58.087 | 229.599 | 1.000 |
| cpu | 4 | smccc | 89.468 | 124.364 | 271.363 | 1.555 |
| getpid | 1 | mmio | 449.217 | 468.689 | 464.534 | 394.603 |
| getpid | 1 | native | 1.138 | 1.621 | 1.518 | 1.000 |
| getpid | 1 | smccc | 350.596 | 365.243 | 364.560 | 307.972 |
| getpid | 4 | mmio | 795.239 | 832.127 | 2373.648 | 595.751 |
| getpid | 4 | native | 1.335 | 1.900 | 5.470 | 1.000 |
| getpid | 4 | smccc | 622.080 | 661.313 | 1852.608 | 466.029 |
| idle | 1 | mmio | 1010.833 | 1058.384 | 51.103 | 1.005 |
| idle | 1 | native | 1006.211 | 1008.808 | 2.378 | 1.000 |
| idle | 1 | smccc | 1006.325 | 1049.819 | 44.897 | 1.000 |
| idle | 4 | mmio | 1022.897 | 1091.344 | 87.380 | 1.013 |
| idle | 4 | native | 1009.593 | 1012.172 | 3.409 | 1.000 |
| idle | 4 | smccc | 1025.147 | 1092.943 | 86.990 | 1.015 |
| mmap | 1 | mmio | 140.552 | 156.517 | 155.600 | 163.730 |
| mmap | 1 | native | 0.858 | 1.305 | 1.191 | 1.000 |
| mmap | 1 | smccc | 122.860 | 137.019 | 136.183 | 143.121 |
| mmap | 4 | mmio | 669.392 | 711.796 | 1687.605 | 71.859 |
| mmap | 4 | native | 9.315 | 9.925 | 14.739 | 1.000 |
| mmap | 4 | smccc | 631.204 | 672.821 | 1608.478 | 67.760 |
| node | 1 | mmio | 68.510 | 4746.951 | 4671.538 | 2.388 |
| node | 1 | native | 28.687 | 131.134 | 129.145 | 1.000 |
| node | 1 | smccc | 62.611 | 4351.471 | 4320.700 | 2.183 |
| node | 4 | mmio | 161.970 | 9772.064 | 26898.058 | 4.492 |
| node | 4 | native | 36.056 | 211.292 | 376.863 | 1.000 |
| node | 4 | smccc | 195.218 | 9464.456 | 26469.462 | 5.414 |
