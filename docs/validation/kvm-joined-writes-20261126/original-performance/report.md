# Paired original workload baseline

Audit: **failed**.
180 measured executions; calibration/warmup excluded.

Medians are milliseconds. Ratio is KVM/native work time (lower is better).
Wall and CPU include startup; sampled memory may miss short peaks.
This host/configuration only; caches retained; no bare-metal claim.

| Workload | Workers | Lane | Work ms | Wall ms | CPU ms | Work/native |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| build | 1 | baseline-mmio | 24150.226 | 24219.051 | 23814.312 | 7.767 |
| build | 1 | candidate-mmio | 22540.895 | 22609.957 | 22316.151 | 7.249 |
| build | 1 | candidate-smccc | 20850.822 | 20916.985 | 20589.769 | 6.706 |
| build | 1 | native | 3109.432 | 3110.366 | 3081.108 | 1.000 |
| build | 4 | baseline-mmio | 31957.512 | 32047.320 | 122506.165 | 32.525 |
| build | 4 | baseline-smccc | 30484.285 | 30572.572 | 115914.721 | 31.026 |
| build | 4 | candidate-mmio | 30644.450 | 30741.988 | 116497.205 | 31.189 |
| build | 4 | candidate-smccc | 28901.261 | 28991.466 | 109519.067 | 29.415 |
| build | 4 | native | 982.549 | 983.406 | 3793.089 | 1.000 |
| cpu | 1 | baseline-mmio | 58.301 | 75.917 | 73.978 | 1.065 |
| cpu | 1 | baseline-smccc | 58.051 | 73.232 | 72.489 | 1.060 |
| cpu | 1 | candidate-mmio | 57.779 | 76.011 | 72.950 | 1.055 |
| cpu | 1 | candidate-smccc | 57.335 | 72.162 | 71.770 | 1.047 |
| cpu | 1 | native | 54.754 | 55.334 | 55.134 | 1.000 |
| cpu | 4 | baseline-mmio | 90.416 | 128.117 | 272.711 | 1.523 |
| cpu | 4 | baseline-smccc | 87.644 | 124.360 | 271.111 | 1.477 |
| cpu | 4 | candidate-mmio | 88.957 | 126.127 | 273.796 | 1.499 |
| cpu | 4 | candidate-smccc | 88.191 | 128.724 | 269.791 | 1.486 |
| cpu | 4 | native | 59.347 | 59.871 | 229.866 | 1.000 |
| getpid | 1 | baseline-mmio | 467.243 | 483.552 | 477.022 | 408.404 |
| getpid | 1 | baseline-smccc | 357.584 | 372.548 | 368.497 | 312.554 |
| getpid | 1 | candidate-mmio | 457.458 | 475.414 | 466.640 | 399.851 |
| getpid | 1 | candidate-smccc | 356.153 | 373.335 | 365.757 | 311.303 |
| getpid | 1 | native | 1.144 | 1.641 | 1.503 | 1.000 |
| getpid | 4 | baseline-mmio | 803.062 | 847.336 | 2367.878 | 603.849 |
| getpid | 4 | baseline-smccc | 629.675 | 668.743 | 1852.678 | 473.473 |
| getpid | 4 | candidate-mmio | 798.294 | 834.621 | 2369.080 | 600.263 |
| getpid | 4 | candidate-smccc | 631.727 | 670.604 | 1842.706 | 475.016 |
| getpid | 4 | native | 1.330 | 1.877 | 5.402 | 1.000 |
| memory | 1 | baseline-mmio | 18.188 | 224.431 | 218.165 | 1.172 |
| memory | 1 | baseline-smccc | 17.917 | 200.428 | 194.862 | 1.155 |
| memory | 1 | candidate-mmio | 17.343 | 219.418 | 213.202 | 1.118 |
| memory | 1 | candidate-smccc | 17.172 | 197.641 | 191.524 | 1.106 |
| memory | 1 | native | 15.519 | 17.094 | 16.600 | 1.000 |
| memory | 4 | baseline-mmio | 176.162 | 1161.735 | 1308.737 | 6.396 |
| memory | 4 | baseline-smccc | 165.629 | 1050.238 | 1217.927 | 6.014 |
| memory | 4 | candidate-mmio | 175.354 | 1108.736 | 1280.721 | 6.367 |
| memory | 4 | candidate-smccc | 142.843 | 991.272 | 1107.732 | 5.187 |
| memory | 4 | native | 27.541 | 34.098 | 109.974 | 1.000 |
| mmap | 1 | baseline-mmio | 150.298 | 166.718 | 162.272 | 172.510 |
| mmap | 1 | baseline-smccc | 129.808 | 145.164 | 140.815 | 148.991 |
| mmap | 1 | candidate-mmio | 150.819 | 167.377 | 162.381 | 173.108 |
| mmap | 1 | candidate-smccc | 130.045 | 149.841 | 142.491 | 149.263 |
| mmap | 1 | native | 0.871 | 1.376 | 1.224 | 1.000 |
| mmap | 4 | baseline-mmio | 686.297 | 724.311 | 1704.897 | 79.579 |
| mmap | 4 | baseline-smccc | 641.390 | 683.052 | 1633.803 | 74.371 |
| mmap | 4 | candidate-mmio | 682.797 | 724.283 | 1715.024 | 79.173 |
| mmap | 4 | candidate-smccc | 643.667 | 686.205 | 1623.491 | 74.636 |
| mmap | 4 | native | 8.624 | 9.235 | 12.963 | 1.000 |
| node | 1 | baseline-mmio | 50.993 | 3681.767 | 3629.549 | 1.894 |
| node | 1 | baseline-smccc | 50.377 | 3431.567 | 3371.662 | 1.871 |
| node | 1 | candidate-mmio | 45.177 | 2901.753 | 2847.392 | 1.678 |
| node | 1 | candidate-smccc | 45.190 | 2619.490 | 2584.838 | 1.678 |
| node | 1 | native | 26.929 | 111.021 | 110.796 | 1.000 |
| node | 4 | baseline-mmio | 148.994 | 8657.113 | 23749.550 | 3.962 |
| node | 4 | baseline-smccc | 152.141 | 8263.206 | 22529.842 | 4.045 |
| node | 4 | candidate-mmio | 143.701 | 7476.485 | 19941.106 | 3.821 |
| node | 4 | candidate-smccc | 120.371 | 7112.902 | 19201.262 | 3.201 |
| node | 4 | native | 37.608 | 207.913 | 369.450 | 1.000 |

## Candidate / baseline time ratios

| Workload | Workers | Transport | Work ratio | Wall ratio |
| --- | ---: | --- | ---: | ---: |
| build | 1 | mmio | 0.933 | 0.934 |
| build | 4 | mmio | 0.959 | 0.959 |
| build | 4 | smccc | 0.948 | 0.948 |
| cpu | 1 | mmio | 0.991 | 1.001 |
| cpu | 1 | smccc | 0.988 | 0.985 |
| cpu | 4 | mmio | 0.984 | 0.984 |
| cpu | 4 | smccc | 1.006 | 1.035 |
| getpid | 1 | mmio | 0.979 | 0.983 |
| getpid | 1 | smccc | 0.996 | 1.002 |
| getpid | 4 | mmio | 0.994 | 0.985 |
| getpid | 4 | smccc | 1.003 | 1.003 |
| memory | 1 | mmio | 0.954 | 0.978 |
| memory | 1 | smccc | 0.958 | 0.986 |
| memory | 4 | mmio | 0.995 | 0.954 |
| memory | 4 | smccc | 0.862 | 0.944 |
| mmap | 1 | mmio | 1.003 | 1.004 |
| mmap | 1 | smccc | 1.002 | 1.032 |
| mmap | 4 | mmio | 0.995 | 1.000 |
| mmap | 4 | smccc | 1.004 | 1.005 |
| node | 1 | mmio | 0.886 | 0.788 |
| node | 1 | smccc | 0.897 | 0.763 |
| node | 4 | mmio | 0.964 | 0.864 |
| node | 4 | smccc | 0.791 | 0.861 |

Failed evidence: build-1-sample-1-baseline-smccc, incomplete distribution: build/1/baseline-smccc
