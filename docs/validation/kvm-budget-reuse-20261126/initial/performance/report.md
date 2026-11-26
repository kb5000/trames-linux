# Paired original workload baseline

Audit: **passed**. 
240 measured executions; calibration/warmup excluded.

Medians are milliseconds. Ratio is KVM/native work time (lower is better).
Wall and CPU include startup; sampled memory may miss short peaks.
This host/configuration only; caches retained; no bare-metal claim.

| Workload | Workers | Lane | Work ms | Wall ms | CPU ms | Work/native |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| build | 1 | baseline-mmio | 22092.632 | 22161.158 | 22157.002 | 7.166 |
| build | 1 | baseline-smccc | 20399.668 | 20465.878 | 20459.058 | 6.617 |
| build | 1 | candidate-mmio | 21276.946 | 21350.195 | 21341.052 | 6.901 |
| build | 1 | candidate-smccc | 19634.330 | 19701.258 | 19696.504 | 6.369 |
| build | 1 | native | 3082.992 | 3083.818 | 3081.336 | 1.000 |
| build | 4 | baseline-mmio | 29270.037 | 29364.754 | 112783.234 | 29.993 |
| build | 4 | baseline-smccc | 28730.375 | 28816.176 | 110992.990 | 29.440 |
| build | 4 | candidate-mmio | 29688.052 | 29773.859 | 114809.778 | 30.422 |
| build | 4 | candidate-smccc | 29320.565 | 29425.445 | 108380.887 | 30.045 |
| build | 4 | native | 975.886 | 976.772 | 3747.450 | 1.000 |
| cpu | 1 | baseline-mmio | 57.361 | 72.809 | 72.426 | 1.059 |
| cpu | 1 | baseline-smccc | 57.223 | 71.665 | 71.297 | 1.056 |
| cpu | 1 | candidate-mmio | 57.222 | 72.420 | 72.043 | 1.056 |
| cpu | 1 | candidate-smccc | 57.576 | 72.088 | 71.728 | 1.063 |
| cpu | 1 | native | 54.168 | 54.557 | 54.449 | 1.000 |
| cpu | 4 | baseline-mmio | 86.922 | 134.555 | 272.208 | 1.504 |
| cpu | 4 | baseline-smccc | 86.825 | 125.152 | 269.128 | 1.502 |
| cpu | 4 | candidate-mmio | 84.740 | 126.131 | 271.958 | 1.466 |
| cpu | 4 | candidate-smccc | 88.164 | 131.442 | 269.315 | 1.525 |
| cpu | 4 | native | 57.795 | 58.337 | 230.035 | 1.000 |
| fork-exec | 1 | baseline-mmio | 182.618 | 199.743 | 199.318 | 132.553 |
| fork-exec | 1 | baseline-smccc | 160.369 | 176.160 | 175.763 | 116.403 |
| fork-exec | 1 | candidate-mmio | 176.994 | 195.006 | 193.334 | 128.470 |
| fork-exec | 1 | candidate-smccc | 154.721 | 170.245 | 169.824 | 112.304 |
| fork-exec | 1 | native | 1.378 | 1.837 | 1.727 | 1.000 |
| fork-exec | 4 | baseline-mmio | 630.085 | 670.662 | 2338.737 | 143.150 |
| fork-exec | 4 | baseline-smccc | 593.287 | 638.646 | 2145.903 | 134.790 |
| fork-exec | 4 | candidate-mmio | 632.628 | 671.845 | 2347.589 | 143.728 |
| fork-exec | 4 | candidate-smccc | 565.708 | 608.922 | 2088.497 | 128.524 |
| fork-exec | 4 | native | 4.402 | 4.980 | 12.347 | 1.000 |
| futex | 1 | baseline-mmio | 186.074 | 203.608 | 203.171 | 29.832 |
| futex | 1 | baseline-smccc | 145.263 | 160.864 | 160.449 | 23.289 |
| futex | 1 | candidate-mmio | 179.751 | 198.324 | 196.064 | 28.818 |
| futex | 1 | candidate-smccc | 140.410 | 155.667 | 155.271 | 22.511 |
| futex | 1 | native | 6.237 | 6.802 | 6.670 | 1.000 |
| futex | 4 | baseline-mmio | 57.462 | 84.388 | 134.259 | 1.725 |
| futex | 4 | baseline-smccc | 45.416 | 71.384 | 108.139 | 1.363 |
| futex | 4 | candidate-mmio | 56.705 | 84.352 | 131.409 | 1.702 |
| futex | 4 | candidate-smccc | 46.026 | 67.371 | 108.075 | 1.382 |
| futex | 4 | native | 33.314 | 33.750 | 16.718 | 1.000 |
| getpid | 1 | baseline-mmio | 446.853 | 463.049 | 462.624 | 392.866 |
| getpid | 1 | baseline-smccc | 349.263 | 364.944 | 363.914 | 307.067 |
| getpid | 1 | candidate-mmio | 434.967 | 450.918 | 450.538 | 382.417 |
| getpid | 1 | candidate-smccc | 336.468 | 354.451 | 350.729 | 295.817 |
| getpid | 1 | native | 1.137 | 1.667 | 1.561 | 1.000 |
| getpid | 4 | baseline-mmio | 778.945 | 814.913 | 2353.337 | 602.919 |
| getpid | 4 | baseline-smccc | 610.682 | 648.112 | 1841.486 | 472.680 |
| getpid | 4 | candidate-mmio | 758.965 | 792.308 | 2297.295 | 587.453 |
| getpid | 4 | candidate-smccc | 586.584 | 620.377 | 1773.087 | 454.028 |
| getpid | 4 | native | 1.292 | 1.855 | 5.386 | 1.000 |
| mmap | 1 | baseline-mmio | 145.502 | 161.545 | 161.169 | 152.545 |
| mmap | 1 | baseline-smccc | 129.000 | 144.072 | 143.653 | 135.244 |
| mmap | 1 | candidate-mmio | 142.181 | 160.096 | 157.991 | 149.063 |
| mmap | 1 | candidate-smccc | 123.562 | 138.612 | 138.221 | 129.543 |
| mmap | 1 | native | 0.954 | 1.418 | 1.305 | 1.000 |
| mmap | 4 | baseline-mmio | 664.151 | 701.958 | 1678.317 | 81.830 |
| mmap | 4 | baseline-smccc | 624.289 | 665.013 | 1604.673 | 76.919 |
| mmap | 4 | candidate-mmio | 650.848 | 693.025 | 1671.300 | 80.191 |
| mmap | 4 | candidate-smccc | 618.121 | 654.937 | 1577.921 | 76.159 |
| mmap | 4 | native | 8.116 | 8.572 | 12.392 | 1.000 |
| node | 1 | baseline-mmio | 44.227 | 2792.178 | 2790.147 | 1.662 |
| node | 1 | baseline-smccc | 43.283 | 2559.550 | 2558.145 | 1.626 |
| node | 1 | candidate-mmio | 44.150 | 2723.794 | 2722.533 | 1.659 |
| node | 1 | candidate-smccc | 43.152 | 2478.604 | 2476.402 | 1.621 |
| node | 1 | native | 26.618 | 110.096 | 109.862 | 1.000 |
| node | 4 | baseline-mmio | 222.666 | 10305.325 | 24017.272 | 6.230 |
| node | 4 | baseline-smccc | 139.070 | 7154.556 | 19720.574 | 3.891 |
| node | 4 | candidate-mmio | 155.217 | 7366.064 | 20212.746 | 4.343 |
| node | 4 | candidate-smccc | 145.543 | 6916.570 | 18864.903 | 4.072 |
| node | 4 | native | 35.741 | 202.935 | 370.381 | 1.000 |
| pipe | 1 | baseline-mmio | 189.024 | 206.547 | 206.158 | 27.987 |
| pipe | 1 | baseline-smccc | 149.395 | 166.695 | 164.853 | 22.119 |
| pipe | 1 | candidate-mmio | 186.009 | 202.699 | 202.354 | 27.540 |
| pipe | 1 | candidate-smccc | 146.243 | 161.863 | 161.461 | 21.653 |
| pipe | 1 | native | 6.754 | 7.264 | 7.139 | 1.000 |
| pipe | 4 | baseline-mmio | 154.324 | 178.252 | 273.894 | 4.543 |
| pipe | 4 | baseline-smccc | 130.304 | 157.664 | 223.937 | 3.836 |
| pipe | 4 | candidate-mmio | 150.097 | 173.764 | 262.996 | 4.419 |
| pipe | 4 | candidate-smccc | 126.837 | 148.710 | 216.045 | 3.734 |
| pipe | 4 | native | 33.966 | 34.458 | 17.299 | 1.000 |

## Candidate / baseline time ratios

| Workload | Workers | Transport | Work ratio | Wall ratio |
| --- | ---: | --- | ---: | ---: |
| build | 1 | mmio | 0.963 | 0.963 |
| build | 1 | smccc | 0.962 | 0.963 |
| build | 4 | mmio | 1.014 | 1.014 |
| build | 4 | smccc | 1.021 | 1.021 |
| cpu | 1 | mmio | 0.998 | 0.995 |
| cpu | 1 | smccc | 1.006 | 1.006 |
| cpu | 4 | mmio | 0.975 | 0.937 |
| cpu | 4 | smccc | 1.015 | 1.050 |
| fork-exec | 1 | mmio | 0.969 | 0.976 |
| fork-exec | 1 | smccc | 0.965 | 0.966 |
| fork-exec | 4 | mmio | 1.004 | 1.002 |
| fork-exec | 4 | smccc | 0.954 | 0.953 |
| futex | 1 | mmio | 0.966 | 0.974 |
| futex | 1 | smccc | 0.967 | 0.968 |
| futex | 4 | mmio | 0.987 | 1.000 |
| futex | 4 | smccc | 1.013 | 0.944 |
| getpid | 1 | mmio | 0.973 | 0.974 |
| getpid | 1 | smccc | 0.963 | 0.971 |
| getpid | 4 | mmio | 0.974 | 0.972 |
| getpid | 4 | smccc | 0.961 | 0.957 |
| mmap | 1 | mmio | 0.977 | 0.991 |
| mmap | 1 | smccc | 0.958 | 0.962 |
| mmap | 4 | mmio | 0.980 | 0.987 |
| mmap | 4 | smccc | 0.990 | 0.985 |
| node | 1 | mmio | 0.998 | 0.976 |
| node | 1 | smccc | 0.997 | 0.968 |
| node | 4 | mmio | 0.697 | 0.715 |
| node | 4 | smccc | 1.047 | 0.967 |
| pipe | 1 | mmio | 0.984 | 0.981 |
| pipe | 1 | smccc | 0.979 | 0.971 |
| pipe | 4 | mmio | 0.973 | 0.975 |
| pipe | 4 | smccc | 0.973 | 0.943 |
