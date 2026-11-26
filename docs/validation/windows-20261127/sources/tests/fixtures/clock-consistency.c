// SPDX-License-Identifier: GPL-3.0-or-later
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

static int64_t ns(clockid_t clock) {
    struct timespec value;
    if (clock_gettime(clock, &value)) { perror("clock_gettime"); exit(2); }
    return (int64_t)value.tv_sec * 1000000000 + value.tv_nsec;
}
int main(int argc, char **argv) {
    if (argc != 3) return 2;
    char *end;
    errno = 0;
    long duration_ms = strtol(argv[1], &end, 10);
    if (errno || *end) return 2;
    if (duration_ms < 100 || duration_ms > 120000) return 2;
    int fd = open(argv[2], O_RDWR | O_CREAT | O_EXCL, 0600);
    if (fd < 0) { perror("open"); return 2; }
    char byte = 1;
    if (pwrite(fd, &byte, 1, 0) != 1) return 2;
    int64_t began = ns(CLOCK_MONOTONIC), old_low = 0, old_high = 0;
    int64_t worst_back = 0, worst_forward = 0, worst_future = 0;
    unsigned samples = 0, backwards = 0, forwards = 0, future = 0;
    while (ns(CLOCK_MONOTONIC) - began < duration_ms * 1000000LL) {
        int64_t before = ns(CLOCK_MONOTONIC);
        int64_t realtime = ns(CLOCK_REALTIME);
        int64_t after = ns(CLOCK_MONOTONIC);
        int64_t low = realtime - after, high = realtime - before;
        int64_t back = samples ? old_low - high : 0;
        int64_t forward = samples ? low - old_high : 0;
        if (back > 1000000) {
            ++backwards;
            if (back > worst_back) worst_back = back;
            printf("{\"event\":\"backward-step\",\"realtime_ns\":%" PRId64 ",\"lower_bound_ns\":%" PRId64 "}\n", realtime, back);
        }
        if (forward > 1000000) {
            ++forwards;
            if (forward > worst_forward) worst_forward = forward;
            printf("{\"event\":\"forward-step\",\"realtime_ns\":%" PRId64 ",\"lower_bound_ns\":%" PRId64 "}\n", realtime, forward);
        }
        old_low = low; old_high = high; ++samples;
        struct stat st;
        if (fstat(fd, &st)) return 2;
        struct timespec raw;
        if (syscall(SYS_clock_gettime, CLOCK_REALTIME, &raw)) return 2;
        int64_t wall = (int64_t)raw.tv_sec * 1000000000 + raw.tv_nsec;
        int64_t stamp = (int64_t)st.st_mtim.tv_sec * 1000000000 + st.st_mtim.tv_nsec;
        int64_t difference = stamp - wall;
        if (difference > 1000000) {
            ++future;
            if (difference > worst_future) worst_future = difference;
        }
        if (pwrite(fd, &byte, 1, 0) != 1) return 2;
        struct timespec delay = { .tv_sec = 0, .tv_nsec = 2000000 };
        while (nanosleep(&delay, &delay)) { if (errno != EINTR) return 2; }
    }
    if (close(fd) || unlink(argv[2])) return 2;
    printf("{\"event\":\"summary\",\"samples\":%u,\"backward_steps\":%u,\"forward_steps\":%u,\"future_timestamps\":%u,\"max_backward_ns\":%" PRId64 ",\"max_forward_ns\":%" PRId64 ",\"max_future_ns\":%" PRId64 "}\n", samples, backwards, forwards, future, worst_back, worst_forward, worst_future);
    return backwards || forwards || future ? 1 : 0;
}
