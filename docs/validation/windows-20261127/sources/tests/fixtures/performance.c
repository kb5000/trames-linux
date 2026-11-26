// SPDX-License-Identifier: GPL-3.0-or-later
// Same original ELF/work for native and KVM; correctness is checked before timing is accepted.
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <linux/futex.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

enum { PAGE = 4096, MEMORY = 8 * 1024 * 1024 };
static const char *mode, *program;
static uint64_t iterations;
static unsigned concurrency;
static pthread_barrier_t start;
static _Atomic unsigned turn;
static int channel[2], response[2];
struct worker { unsigned index; uint64_t checksum; uint64_t *memory; };
static uint64_t now(void) {
    struct timespec t; assert(clock_gettime(CLOCK_MONOTONIC, &t) == 0);
    return (uint64_t)t.tv_sec * 1000000000 + (uint64_t)t.tv_nsec;
}
static void barrier(void) {
    int result = pthread_barrier_wait(&start);
    assert(result == 0 || result == PTHREAD_BARRIER_SERIAL_THREAD);
}
static void transfer(int fd, void *bytes, size_t size, int write) {
    size_t done = 0;
    while (done < size) {
        ssize_t n = write ? syscall(SYS_write, fd, (char *)bytes + done, size - done)
                          : syscall(SYS_read, fd, (char *)bytes + done, size - done);
        if (n < 0 && errno == EINTR) continue;
        assert(n > 0); done += (size_t)n;
    }
}
static void child_status(pid_t pid) {
    int status; assert(waitpid(pid, &status, 0) == pid);
    assert(WIFEXITED(status) && WEXITSTATUS(status) == 0);
}
static uint64_t read_hash(const char *path) {
    int fd = open(path, O_RDONLY); assert(fd >= 0); unsigned char bytes[4096];
    uint64_t hash = UINT64_C(1469598103934665603);
    for (;;) {
        ssize_t n = read(fd, bytes, sizeof(bytes)); assert(n >= 0); if (!n) break;
        for (ssize_t i = 0; i < n; i++) hash = (hash ^ bytes[i]) * UINT64_C(1099511628211);
    }
    assert(close(fd) == 0); return hash;
}
static void *work(void *argument) {
    struct worker *w = argument; uint64_t value = w->index + 1;
    unsigned char packet[PAGE]; memset(packet, (int)w->index + 1, sizeof(packet));
    long pid = syscall(SYS_getpid); assert(pid > 0);
    barrier();
    if (!strcmp(mode, "cpu")) {
        for (uint64_t n = 0; n < iterations; n++) value = (value * UINT64_C(2862933555777941757) + 3037000493u) ^ (value >> 17);
    } else if (!strcmp(mode, "memory")) {
        for (uint64_t n = 0; n < iterations; n++)
            for (unsigned i = 0; i < MEMORY / sizeof(uint64_t); i++) w->memory[i] += 1;
        assert(w->memory[0] == w->index + iterations);
        value = w->memory[0] ^ w->memory[MEMORY / sizeof(uint64_t) - 1];
    } else if (!strcmp(mode, "getpid")) {
        for (uint64_t n = 0; n < iterations; n++) assert(syscall(SYS_getpid) == pid);
        value = iterations;
    } else if (!strcmp(mode, "file-4k")) {
        char name[64]; snprintf(name, sizeof(name), "file-%u", w->index);
        int fd = open(name, O_CREAT | O_TRUNC | O_RDWR, 0600); assert(fd >= 0);
        unsigned char out[PAGE];
        for (uint64_t n = 0; n < iterations; n++) {
            memcpy(packet, &n, sizeof(n)); off_t offset = (off_t)(n % 256) * PAGE;
            assert(pwrite(fd, packet, PAGE, offset) == PAGE && pread(fd, out, PAGE, offset) == PAGE);
            assert(!memcmp(packet, out, PAGE));
        }
        assert(close(fd) == 0 && unlink(name) == 0); value = iterations;
    } else if (!strcmp(mode, "pipe") || !strcmp(mode, "unix-socket")) {
        for (uint64_t n = 0; n < iterations; n++) {
            uint64_t ack = n;
            if (w->index == 0) {
                memcpy(packet, &n, sizeof(n)); transfer(channel[1], packet, PAGE, 1);
                transfer(response[0], &ack, sizeof(ack), 0); assert(ack == n);
            } else {
                transfer(channel[0], packet, PAGE, 0); memcpy(&ack, packet, sizeof(ack)); assert(ack == n);
                transfer(response[1], &ack, sizeof(ack), 1);
            }
        }
        value = iterations;
    } else if (!strcmp(mode, "futex")) {
        for (uint64_t n = 0; n < iterations; n++) {
            for (;;) {
                unsigned observed = atomic_load_explicit(&turn, memory_order_acquire);
                if (observed == w->index) break;
                int result = (int)syscall(SYS_futex, &turn, FUTEX_WAIT_PRIVATE, observed, NULL, NULL, 0);
                assert(result == 0 || (result == -1 && (errno == EAGAIN || errno == EINTR)));
            }
            atomic_store_explicit(&turn, 1 - w->index, memory_order_release);
            assert(syscall(SYS_futex, &turn, FUTEX_WAKE_PRIVATE, 1, NULL, NULL, 0) >= 0);
        }
        value = iterations;
    } else if (!strcmp(mode, "mmap")) {
        for (uint64_t n = 0; n < iterations; n++) {
            unsigned char *p = mmap(NULL, 3 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
            assert(p != MAP_FAILED); p[0] = 3; p[PAGE] = 5; p[3 * PAGE - 1] = 7;
            assert(mprotect(p + PAGE, PAGE, PROT_READ) == 0);
            assert(p[0] + p[PAGE] + p[3 * PAGE - 1] == 15); assert(munmap(p, 3 * PAGE) == 0);
        }
        value = iterations;
    } else if (!strcmp(mode, "cow")) {
        for (uint64_t n = 0; n < iterations; n++) {
            pid_t child = fork(); assert(child >= 0);
            if (!child) {
                for (unsigned i = 0; i < MEMORY / sizeof(uint64_t); i += PAGE / sizeof(uint64_t)) w->memory[i] ^= 0x75;
                _exit(0);
            }
            child_status(child); assert(w->memory[0] == w->index);
        }
        value = iterations;
    } else if (!strcmp(mode, "fork-exec")) {
        for (uint64_t n = 0; n < iterations; n++) {
            pid_t child = fork(); assert(child >= 0);
            if (!child) { execl(program, program, "child", (char *)NULL); _exit(127); }
            child_status(child);
        }
        value = iterations;
    } else if (!strcmp(mode, "idle")) {
        struct timespec pause = {.tv_sec = (time_t)(iterations / 1000), .tv_nsec = (long)(iterations % 1000) * 1000000};
        while (nanosleep(&pause, &pause) != 0) assert(errno == EINTR);
        value = iterations;
    } else abort();
    w->checksum = value; return NULL;
}
static uint64_t build_prepare(void) {
    FILE *make = fopen("Makefile", "w"); assert(make);
    assert(fprintf(make, "CC=/usr/bin/cc\nall:") > 0);
    for (uint64_t n = 0; n < iterations; n++) assert(fprintf(make, " unit-%" PRIu64 ".o", n) > 0);
    assert(fputc('\n', make) != EOF);
    for (uint64_t n = 0; n < iterations; n++) {
        char name[80]; snprintf(name, sizeof(name), "unit-%" PRIu64 ".c", n);
        FILE *source = fopen(name, "w"); assert(source);
        assert(fprintf(source, "unsigned long long f%" PRIu64 "(unsigned long long x) {\n", n) > 0);
        for (unsigned line = 0; line < 256; line++) assert(fprintf(source, "x=(x*%uu+%uu)^(x>>%u);\n", 1664525u + line, 1013904223u + (unsigned)n, line % 13 + 1) > 0);
        assert(fputs("return x;}\n", source) >= 0 && fclose(source) == 0);
        assert(fprintf(make, "unit-%" PRIu64 ".o: unit-%" PRIu64 ".c\n\t$(CC) -O2 -fno-ident -frandom-seed=trames -nostdinc -c $< -o $@\n", n, n) > 0);
    }
    assert(fclose(make) == 0); return now();
}
int main(int argc, char **argv) {
    assert(sysconf(_SC_PAGESIZE) == PAGE);
    if (argc == 2 && !strcmp(argv[1], "child")) return 0;
    assert(argc == 5); mode = argv[1]; iterations = strtoull(argv[2], NULL, 10);
    concurrency = (unsigned)strtoul(argv[3], NULL, 10); program = argv[0];
    assert(iterations && concurrency && concurrency <= 16); assert(chdir(argv[4]) == 0);
    uint64_t began, elapsed, checksum = 0; unsigned tasks = concurrency;
    if (!strcmp(mode, "build")) {
        began = build_prepare(); char jobs[24]; snprintf(jobs, sizeof(jobs), "-j%u", concurrency);
        pid_t child = fork(); assert(child >= 0);
        if (!child) { execl("/usr/bin/make", "make", "-s", "-B", "--no-print-directory", jobs, (char *)NULL); _exit(127); }
        child_status(child); elapsed = now() - began;
        for (uint64_t n = 0; n < iterations; n++) {
            char name[80]; snprintf(name, sizeof(name), "unit-%" PRIu64 ".o", n); checksum ^= read_hash(name);
        }
    } else {
        if (!strcmp(mode, "pipe") || !strcmp(mode, "unix-socket") || !strcmp(mode, "futex")) tasks = 2;
        if (!strcmp(mode, "pipe")) { assert(pipe(channel) == 0 && pipe(response) == 0); }
        if (!strcmp(mode, "unix-socket")) {
            assert(socketpair(AF_UNIX, SOCK_STREAM, 0, channel) == 0 && socketpair(AF_UNIX, SOCK_STREAM, 0, response) == 0);
        }
        assert(pthread_barrier_init(&start, NULL, tasks + 1) == 0);
        pthread_t threads[16]; struct worker workers[16] = {0};
        for (unsigned i = 0; i < tasks; i++) {
            workers[i].index = i;
            if (!strcmp(mode, "memory") || !strcmp(mode, "cow")) {
                workers[i].memory = aligned_alloc(PAGE, MEMORY); assert(workers[i].memory);
                for (unsigned j = 0; j < MEMORY / sizeof(uint64_t); j++) workers[i].memory[j] = j + i;
            }
            assert(pthread_create(&threads[i], NULL, work, &workers[i]) == 0);
        }
        began = now(); barrier();
        for (unsigned i = 0; i < tasks; i++) assert(pthread_join(threads[i], NULL) == 0);
        elapsed = now() - began;
        for (unsigned i = 0; i < tasks; i++) { checksum += workers[i].checksum; free(workers[i].memory); }
        assert(pthread_barrier_destroy(&start) == 0);
    }
    printf("{\"mode\":\"%s\",\"concurrency\":%u,\"tasks\":%u,\"iterations\":%" PRIu64 ",\"checksum\":%" PRIu64 ",\"work_ns\":%" PRIu64 "}\n", mode, concurrency, tasks, iterations, checksum, elapsed);
    return 0;
}
