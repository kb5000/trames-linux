# Paired original workload baseline

Audit: **passed**. 
240 measured executions; calibration/warmup excluded.

Medians are milliseconds. Ratio is KVM/native work time (lower is better).
Wall and CPU include startup; sampled memory may miss short peaks.
This host/configuration only; caches retained; no bare-metal claim.

| Workload | Workers | Lane | Work ms | Wall ms | CPU ms | Work/native |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| build | 1 | baseline-mmio | 22097.793 | 22168.238 | 22156.863 | 7.177 |
| build | 1 | baseline-smccc | 20360.289 | 20424.530 | 20416.979 | 6.613 |
| build | 1 | candidate-mmio | 21246.080 | 21313.547 | 21307.427 | 6.900 |
| build | 1 | candidate-smccc | 19513.041 | 19577.350 | 19566.088 | 6.338 |
| build | 1 | native | 3078.953 | 3079.773 | 3079.420 | 1.000 |
| build | 4 | baseline-mmio | 29722.759 | 29808.610 | 114972.086 | 30.264 |
| build | 4 | baseline-smccc | 29219.846 | 29305.607 | 113154.595 | 29.752 |
| build | 4 | candidate-mmio | 29509.882 | 29596.370 | 114593.595 | 30.047 |
| build | 4 | candidate-smccc | 29017.580 | 29100.081 | 112163.808 | 29.546 |
| build | 4 | native | 982.113 | 982.997 | 3749.451 | 1.000 |
| cpu | 1 | baseline-mmio | 57.382 | 72.914 | 72.538 | 1.061 |
| cpu | 1 | baseline-smccc | 57.150 | 71.567 | 71.230 | 1.056 |
| cpu | 1 | candidate-mmio | 57.312 | 72.154 | 71.745 | 1.059 |
| cpu | 1 | candidate-smccc | 57.040 | 70.970 | 70.642 | 1.054 |
| cpu | 1 | native | 54.095 | 54.497 | 54.369 | 1.000 |
| cpu | 4 | baseline-mmio | 87.307 | 133.486 | 272.241 | 1.499 |
| cpu | 4 | baseline-smccc | 86.060 | 127.131 | 269.739 | 1.478 |
| cpu | 4 | candidate-mmio | 87.630 | 128.072 | 272.333 | 1.505 |
| cpu | 4 | candidate-smccc | 88.486 | 124.247 | 268.039 | 1.519 |
| cpu | 4 | native | 58.237 | 58.769 | 229.650 | 1.000 |
| fork-exec | 1 | baseline-mmio | 180.988 | 197.979 | 197.658 | 132.015 |
| fork-exec | 1 | baseline-smccc | 160.647 | 175.903 | 175.527 | 117.179 |
| fork-exec | 1 | candidate-mmio | 175.207 | 191.476 | 191.101 | 127.799 |
| fork-exec | 1 | candidate-smccc | 154.637 | 169.454 | 169.056 | 112.795 |
| fork-exec | 1 | native | 1.371 | 1.843 | 1.729 | 1.000 |
| fork-exec | 4 | baseline-mmio | 619.071 | 662.440 | 2324.513 | 154.689 |
| fork-exec | 4 | baseline-smccc | 595.675 | 637.117 | 2157.772 | 148.843 |
| fork-exec | 4 | candidate-mmio | 636.233 | 683.430 | 2372.483 | 158.977 |
| fork-exec | 4 | candidate-smccc | 576.655 | 614.747 | 2108.347 | 144.090 |
| fork-exec | 4 | native | 4.002 | 4.603 | 12.119 | 1.000 |
| futex | 1 | baseline-mmio | 191.718 | 209.567 | 209.221 | 30.303 |
| futex | 1 | baseline-smccc | 144.081 | 160.067 | 159.693 | 22.774 |
| futex | 1 | candidate-mmio | 179.457 | 196.074 | 195.693 | 28.365 |
| futex | 1 | candidate-smccc | 139.980 | 155.650 | 155.275 | 22.126 |
| futex | 1 | native | 6.327 | 6.877 | 6.725 | 1.000 |
| futex | 4 | baseline-mmio | 57.323 | 88.535 | 134.181 | 1.721 |
| futex | 4 | baseline-smccc | 45.365 | 71.370 | 107.801 | 1.362 |
| futex | 4 | candidate-mmio | 57.426 | 80.279 | 133.225 | 1.724 |
| futex | 4 | candidate-smccc | 44.554 | 66.452 | 104.737 | 1.338 |
| futex | 4 | native | 33.302 | 33.776 | 16.743 | 1.000 |
| getpid | 1 | baseline-mmio | 445.997 | 462.146 | 461.694 | 391.211 |
| getpid | 1 | baseline-smccc | 348.657 | 363.358 | 362.955 | 305.828 |
| getpid | 1 | candidate-mmio | 432.939 | 452.120 | 448.407 | 379.757 |
| getpid | 1 | candidate-smccc | 334.894 | 352.646 | 349.104 | 293.755 |
| getpid | 1 | native | 1.140 | 1.644 | 1.544 | 1.000 |
| getpid | 4 | baseline-mmio | 778.022 | 824.043 | 2345.551 | 594.193 |
| getpid | 4 | baseline-smccc | 610.135 | 648.900 | 1842.362 | 465.974 |
| getpid | 4 | candidate-mmio | 761.664 | 802.031 | 2293.388 | 581.700 |
| getpid | 4 | candidate-smccc | 585.372 | 616.919 | 1762.857 | 447.062 |
| getpid | 4 | native | 1.309 | 1.877 | 5.476 | 1.000 |
| mmap | 1 | baseline-mmio | 145.514 | 161.342 | 160.974 | 159.994 |
| mmap | 1 | baseline-smccc | 129.040 | 143.654 | 143.283 | 141.880 |
| mmap | 1 | candidate-mmio | 139.617 | 154.939 | 154.541 | 153.509 |
| mmap | 1 | candidate-smccc | 123.017 | 136.973 | 136.592 | 135.258 |
| mmap | 1 | native | 0.909 | 1.375 | 1.277 | 1.000 |
| mmap | 4 | baseline-mmio | 664.638 | 705.073 | 1682.183 | 81.949 |
| mmap | 4 | baseline-smccc | 629.592 | 666.298 | 1594.125 | 77.628 |
| mmap | 4 | candidate-mmio | 651.344 | 696.907 | 1679.604 | 80.310 |
| mmap | 4 | candidate-smccc | 622.730 | 660.766 | 1585.788 | 76.782 |
| mmap | 4 | native | 8.110 | 8.692 | 12.451 | 1.000 |
| node | 1 | baseline-mmio | 44.123 | 2792.766 | 2791.694 | 1.649 |
| node | 1 | baseline-smccc | 42.983 | 2517.563 | 2516.274 | 1.607 |
| node | 1 | candidate-mmio | 44.294 | 2713.652 | 2712.418 | 1.656 |
| node | 1 | candidate-smccc | 43.066 | 2457.273 | 2452.630 | 1.610 |
| node | 1 | native | 26.753 | 109.772 | 109.557 | 1.000 |
| node | 4 | baseline-mmio | 142.051 | 7267.964 | 19533.808 | 3.636 |
| node | 4 | baseline-smccc | 134.082 | 7089.306 | 19098.066 | 3.432 |
| node | 4 | candidate-mmio | 137.001 | 7277.025 | 19828.676 | 3.506 |
| node | 4 | candidate-smccc | 119.692 | 6836.966 | 18683.924 | 3.063 |
| node | 4 | native | 39.073 | 209.750 | 368.727 | 1.000 |
| pipe | 1 | baseline-mmio | 187.487 | 204.658 | 204.262 | 28.244 |
| pipe | 1 | baseline-smccc | 148.628 | 165.023 | 164.595 | 22.390 |
| pipe | 1 | candidate-mmio | 184.462 | 201.204 | 200.869 | 27.788 |
| pipe | 1 | candidate-smccc | 144.902 | 160.234 | 159.841 | 21.829 |
| pipe | 1 | native | 6.638 | 7.154 | 6.984 | 1.000 |
| pipe | 4 | baseline-mmio | 155.947 | 190.354 | 276.216 | 4.591 |
| pipe | 4 | baseline-smccc | 131.037 | 157.672 | 224.885 | 3.858 |
| pipe | 4 | candidate-mmio | 153.666 | 182.670 | 270.630 | 4.524 |
| pipe | 4 | candidate-smccc | 129.085 | 158.665 | 220.355 | 3.800 |
| pipe | 4 | native | 33.966 | 34.505 | 17.318 | 1.000 |

## Candidate / baseline time ratios

| Workload | Workers | Transport | Work ratio | Wall ratio |
| --- | ---: | --- | ---: | ---: |
| build | 1 | mmio | 0.961 | 0.961 |
| build | 1 | smccc | 0.958 | 0.959 |
| build | 4 | mmio | 0.993 | 0.993 |
| build | 4 | smccc | 0.993 | 0.993 |
| cpu | 1 | mmio | 0.999 | 0.990 |
| cpu | 1 | smccc | 0.998 | 0.992 |
| cpu | 4 | mmio | 1.004 | 0.959 |
| cpu | 4 | smccc | 1.028 | 0.977 |
| fork-exec | 1 | mmio | 0.968 | 0.967 |
| fork-exec | 1 | smccc | 0.963 | 0.963 |
| fork-exec | 4 | mmio | 1.028 | 1.032 |
| fork-exec | 4 | smccc | 0.968 | 0.965 |
| futex | 1 | mmio | 0.936 | 0.936 |
| futex | 1 | smccc | 0.972 | 0.972 |
| futex | 4 | mmio | 1.002 | 0.907 |
| futex | 4 | smccc | 0.982 | 0.931 |
| getpid | 1 | mmio | 0.971 | 0.978 |
| getpid | 1 | smccc | 0.961 | 0.971 |
| getpid | 4 | mmio | 0.979 | 0.973 |
| getpid | 4 | smccc | 0.959 | 0.951 |
| mmap | 1 | mmio | 0.959 | 0.960 |
| mmap | 1 | smccc | 0.953 | 0.953 |
| mmap | 4 | mmio | 0.980 | 0.988 |
| mmap | 4 | smccc | 0.989 | 0.992 |
| node | 1 | mmio | 1.004 | 0.972 |
| node | 1 | smccc | 1.002 | 0.976 |
| node | 4 | mmio | 0.964 | 1.001 |
| node | 4 | smccc | 0.893 | 0.964 |
| pipe | 1 | mmio | 0.984 | 0.983 |
| pipe | 1 | smccc | 0.975 | 0.971 |
| pipe | 4 | mmio | 0.985 | 0.960 |
| pipe | 4 | smccc | 0.985 | 1.006 |
