// SPDX-License-Identifier: GPL-3.0-or-later
// Original Linux ELF; compile all LOAD segments with 4096-byte alignment.
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <setjmp.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

enum { PAGE = 4096 };
struct arena { unsigned char *mapping, *base; size_t length; };
static struct arena reserve(size_t pages) {
    struct arena a = {.length = pages * PAGE + 2 * 65536};
    a.mapping = mmap(NULL, a.length, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    assert(a.mapping != MAP_FAILED);
    a.base = (unsigned char *)(((uintptr_t)a.mapping + 65535) & ~(uintptr_t)65535) + PAGE;
    assert((uintptr_t)a.base % 65536 == PAGE && (uintptr_t)a.base % 16384 == PAGE);
    assert(a.base + pages * PAGE <= a.mapping + a.length);
    return a;
}
static void release(struct arena a) { assert(munmap(a.mapping, a.length) == 0); }
static unsigned char *replace(void *at, size_t length, int flags, int fd, off_t offset) {
    void *p = mmap(at, length, PROT_READ | PROT_WRITE, flags | MAP_FIXED, fd, offset);
    assert(p == at); return p;
}
static int file(size_t bytes, unsigned flags) {
    int fd = memfd_create("trames-4k", flags); assert(fd >= 0);
    assert(ftruncate(fd, (off_t)bytes) == 0); return fd;
}
static _Thread_local sigjmp_buf fault_return;
static _Thread_local volatile sig_atomic_t expecting, caught_signal, caught_code;
static _Thread_local void *volatile caught_address;
static void fault(int signal, siginfo_t *info, void *context) {
    (void)context;
    if (!expecting) _exit(128 + signal);
    caught_signal = signal; caught_code = info->si_code; caught_address = info->si_addr;
    siglongjmp(fault_return, 1);
}
__attribute__((noinline)) static void load_byte(void *p) { (void)*(volatile unsigned char *)p; }
__attribute__((noinline)) static void store_byte(void *p) { *(volatile unsigned char *)p = 0x77; }
static void expect_fault(void *p, int write, int signal, int code) {
    caught_signal = caught_code = 0; caught_address = NULL; expecting = 1;
    if (sigsetjmp(fault_return, 1) == 0) {
        if (write) store_byte(p); else load_byte(p);
        abort();
    }
    expecting = 0;
    if (caught_signal != signal || caught_code != code || caught_address != p)
        fprintf(stderr, "fault: signal=%d code=%d address=%p; expected %d/%d/%p\n",
                caught_signal, caught_code, (void *)caught_address, signal, code, p);
    assert(caught_signal == signal && caught_code == code && caught_address == p);
}
static void geometry(void) {
    struct arena a = reserve(4);
    for (unsigned i = 0; i < 4; i++) {
        unsigned char *p = replace(a.base + i * PAGE, PAGE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        for (unsigned j = 0; j < PAGE; j++) assert(p[j] == 0);
        memset(p, (int)i + 1, PAGE);
    }
    for (unsigned i = 0; i < 4; i++) for (unsigned j = 0; j < PAGE; j++)
        assert(a.base[i * PAGE + j] == i + 1);
    expect_fault(a.base - 1, 0, SIGSEGV, SEGV_ACCERR);
    expect_fault(a.base + 4 * PAGE, 1, SIGSEGV, SEGV_ACCERR); release(a);
}
static void protection(void) {
    struct arena a = reserve(3); replace(a.base, 3 * PAGE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    memset(a.base, 0x19, 3 * PAGE);
    assert(mprotect(a.base + PAGE, PAGE, PROT_READ) == 0);
    expect_fault(a.base + PAGE + 37, 1, SIGSEGV, SEGV_ACCERR);
    a.base[0] = 0x21; a.base[3 * PAGE - 1] = 0x43;
    assert(mprotect(a.base + PAGE, PAGE, PROT_NONE) == 0);
    expect_fault(a.base + PAGE + 81, 0, SIGSEGV, SEGV_ACCERR);
    assert(mprotect(a.base + PAGE, PAGE, PROT_READ | PROT_WRITE) == 0);
    assert(a.base[PAGE + 37] == 0x19 && a.base[0] == 0x21 && a.base[3 * PAGE - 1] == 0x43);
    release(a);
}
static void holes(void) {
    struct arena a = reserve(3); replace(a.base, 3 * PAGE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    memset(a.base, 0x2b, 3 * PAGE); assert(munmap(a.base + PAGE, PAGE) == 0);
    expect_fault(a.base + PAGE + 9, 0, SIGSEGV, SEGV_MAPERR);
    void *p = mmap(a.base, PAGE, PROT_READ, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE, -1, 0);
    assert(p == MAP_FAILED && errno == EEXIST);
    p = mmap(a.base + PAGE, PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE, -1, 0);
    assert(p == a.base + PAGE);
    for (unsigned i = 0; i < PAGE; i++) assert(a.base[PAGE + i] == 0);
    assert(a.base[PAGE - 1] == 0x2b && a.base[2 * PAGE] == 0x2b); release(a);
}
static void owners(void) {
    struct arena a = reserve(3); int first = file(PAGE, 0), second = file(PAGE, 0);
    unsigned char *left = replace(a.base, PAGE, MAP_SHARED, first, 0);
    unsigned char *middle = replace(a.base + PAGE, PAGE, MAP_SHARED, second, 0);
    unsigned char *right = replace(a.base + 2 * PAGE, PAGE, MAP_SHARED, first, 0);
    left[17] = 0x39; middle[17] = 0x67;
    assert(right[17] == 0x39 && middle[17] == 0x67);
    replace(middle, PAGE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    assert(middle[17] == 0 && left[17] == 0x39 && right[17] == 0x39);
    unsigned char value; assert(pread(second, &value, 1, 17) == 1 && value == 0x67);
    assert(close(first) == 0 && close(second) == 0);
    right[17] = 0x72; assert(left[17] == 0x72 && middle[17] == 0); release(a);
}
static void file_offsets(void) {
    int fd = file(4 * PAGE, 0); struct arena a = reserve(3);
    unsigned char *shared = replace(a.base, 2 * PAGE, MAP_SHARED, fd, PAGE);
    unsigned char *alias = replace(a.base + 2 * PAGE, PAGE, MAP_SHARED, fd, 2 * PAGE);
    uint64_t value = UINT64_C(0x123456789abcdef0), got;
    assert(pwrite(fd, &value, sizeof(value), 2 * PAGE - 4) == sizeof(value));
    memcpy(&got, shared + PAGE - 4, sizeof(got)); assert(got == value);
    alias[19] = 0x6a; assert(shared[PAGE + 19] == 0x6a);
    assert(msync(shared, 2 * PAGE, MS_SYNC) == 0);
    unsigned char *private = mmap(NULL, PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE, fd, 2 * PAGE);
    assert(private != MAP_FAILED); private[19] = 0x5c;
    assert(alias[19] == 0x6a);
    unsigned char byte; assert(pread(fd, &byte, 1, 2 * PAGE + 19) == 1 && byte == 0x6a);
    assert(close(fd) == 0); alias[20] = 0x55; assert(shared[PAGE + 20] == 0x55);
    assert(munmap(private, PAGE) == 0); release(a);
}
static void truncate_faults(void) {
    int fd = file(PAGE + 37, 0); struct arena a = reserve(4);
    unsigned char *p = replace(a.base, 4 * PAGE, MAP_SHARED, fd, 0);
    p[PAGE + 36] = 0x41;
    for (unsigned i = 37; i < PAGE; i++) assert(p[PAGE + i] == 0);
    expect_fault(p + 2 * PAGE, 0, SIGBUS, BUS_ADRERR);
    assert(ftruncate(fd, 3 * PAGE) == 0); assert(p[2 * PAGE] == 0); p[2 * PAGE] = 0x51;
    assert(ftruncate(fd, PAGE) == 0);
    expect_fault(p + PAGE, 0, SIGBUS, BUS_ADRERR);
    expect_fault(p + 2 * PAGE, 1, SIGBUS, BUS_ADRERR);
    assert(close(fd) == 0); release(a);
}
static void discard_remap(void) {
    struct arena a = reserve(3); unsigned char *p = replace(a.base, 3 * PAGE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    memset(p, 0x11, PAGE); memset(p + PAGE, 0x22, PAGE); memset(p + 2 * PAGE, 0x33, PAGE);
    assert(madvise(p + PAGE, PAGE, MADV_DONTNEED) == 0);
    for (unsigned i = 0; i < PAGE; i++) assert(p[i] == 0x11 && p[PAGE + i] == 0 && p[2 * PAGE + i] == 0x33);
    p = mremap(p, 3 * PAGE, 5 * PAGE, MREMAP_MAYMOVE); assert(p != MAP_FAILED);
    assert(p[0] == 0x11 && p[PAGE] == 0 && p[2 * PAGE] == 0x33);
    for (unsigned i = 3 * PAGE; i < 5 * PAGE; i++) assert(p[i] == 0);
    unsigned char resident[5]; assert(mincore(p, 5 * PAGE, resident) == 0);
    assert(resident[0] & 1); errno = 0;
    assert(mincore(p + 1, PAGE, resident) == -1 && errno == EINVAL);
    p = mremap(p, 5 * PAGE, 2 * PAGE, 0); assert(p != MAP_FAILED);
    assert(p[0] == 0x11 && p[PAGE] == 0); assert(munmap(p, 2 * PAGE) == 0); release(a);
}
static void cow(void) {
    struct arena a = reserve(3); unsigned char *p = replace(a.base, 3 * PAGE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    unsigned char *shared = mmap(NULL, PAGE, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANONYMOUS, -1, 0);
    assert(shared != MAP_FAILED); memset(p, 0x29, 3 * PAGE);
    assert(mprotect(p + PAGE, PAGE, PROT_READ) == 0);
    pid_t child = fork(); assert(child >= 0);
    if (!child) {
        assert(mprotect(p + PAGE, PAGE, PROT_READ | PROT_WRITE) == 0);
        p[PAGE + 7] = 0x43; shared[0] = 0x61; assert(munmap(p, PAGE) == 0); _exit(0);
    }
    int status; assert(waitpid(child, &status, 0) == child && WIFEXITED(status) && WEXITSTATUS(status) == 0);
    assert(shared[0] == 0x61 && p[0] == 0x29 && p[PAGE + 7] == 0x29);
    expect_fault(p + PAGE + 7, 1, SIGSEGV, SEGV_ACCERR);
    assert(munmap(shared, PAGE) == 0); release(a);
}
static unsigned char *thread_base;
static pthread_barrier_t thread_barrier;
static void *thread_memory(void *argument) {
    uintptr_t i = (uintptr_t)argument; unsigned char *p = thread_base + i * 4 * PAGE;
    int barrier = pthread_barrier_wait(&thread_barrier);
    assert(barrier == 0 || barrier == PTHREAD_BARRIER_SERIAL_THREAD);
    for (unsigned turn = 0; turn < 128; turn++) {
        replace(p, PAGE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0); assert(p[37] == 0);
        p[37] = (unsigned char)i + 1;
        assert(mprotect(p, PAGE, PROT_READ) == 0); expect_fault(p + 37, 1, SIGSEGV, SEGV_ACCERR);
        assert(p[37] == i + 1); assert(munmap(p, PAGE) == 0);
    }
    replace(p, PAGE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0); p[37] = (unsigned char)i + 1;
    return NULL;
}
static void threads(void) {
    struct arena a = reserve(16); thread_base = a.base; pthread_t tasks[4];
    assert(pthread_barrier_init(&thread_barrier, NULL, 4) == 0);
    for (uintptr_t i = 0; i < 4; i++) assert(pthread_create(&tasks[i], NULL, thread_memory, (void *)i) == 0);
    for (unsigned i = 0; i < 4; i++) assert(pthread_join(tasks[i], NULL) == 0);
    for (unsigned i = 0; i < 4; i++) assert(a.base[i * 4 * PAGE + 37] == i + 1);
    assert(pthread_barrier_destroy(&thread_barrier) == 0); release(a);
}
static void retire(void) {
    struct rlimit limit; assert(getrlimit(RLIMIT_AS, &limit) == 0);
    if (limit.rlim_cur > 64 * 1024 * 1024) { limit.rlim_cur = 64 * 1024 * 1024; assert(setrlimit(RLIMIT_AS, &limit) == 0); }
    for (unsigned round = 0; round < 256; round++) {
        unsigned char *p = mmap(NULL, 64 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        assert(p != MAP_FAILED);
        for (unsigned page = 0; page < 64; page++) { assert(p[page * PAGE] == 0); p[page * PAGE] = (unsigned char)(round + 1); }
        assert(munmap(p, 64 * PAGE) == 0);
    }
}
static void exec_child(char **argv) {
    unsigned char resident; void *old = (void *)(uintptr_t)strtoull(argv[2], NULL, 0);
    errno = 0; assert(mincore(old, PAGE, &resident) == -1 && errno == ENOMEM);
    int kept = atoi(argv[3]), closed = atoi(argv[4]); errno = 0;
    assert(fcntl(closed, F_GETFD) == -1 && errno == EBADF);
    unsigned char *p = mmap(NULL, PAGE, PROT_READ, MAP_SHARED, kept, PAGE);
    assert(p != MAP_FAILED && p[0] == 0x79); assert(munmap(p, PAGE) == 0);
}
static void exec_state(const char *program) {
    void *address = (void *)(uintptr_t)0x2000001000ULL;
    void *p = mmap(address, PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE, -1, 0);
    assert(p == address); *(unsigned char *)p = 0x17;
    int kept = file(2 * PAGE, 0), closed = file(PAGE, MFD_CLOEXEC); unsigned char value = 0x79;
    assert(pwrite(kept, &value, 1, PAGE) == 1);
    char pointer[32], kept_text[24], closed_text[24];
    snprintf(pointer, sizeof(pointer), "%#llx", (unsigned long long)(uintptr_t)p);
    snprintf(kept_text, sizeof(kept_text), "%d", kept); snprintf(closed_text, sizeof(closed_text), "%d", closed);
    pid_t child = fork(); assert(child >= 0);
    if (!child) { execl(program, program, "exec-child", pointer, kept_text, closed_text, (char *)NULL); _exit(127); }
    int status; assert(waitpid(child, &status, 0) == child && WIFEXITED(status) && WEXITSTATUS(status) == 0);
    assert(*(unsigned char *)p == 0x17); assert(munmap(p, PAGE) == 0); close(kept); close(closed);
}
int main(int argc, char **argv) {
    assert(argc >= 2 && sysconf(_SC_PAGESIZE) == PAGE && getauxval(AT_PAGESZ) == PAGE);
    struct sigaction action = {.sa_sigaction = fault, .sa_flags = SA_SIGINFO}; sigemptyset(&action.sa_mask);
    assert(sigaction(SIGSEGV, &action, NULL) == 0 && sigaction(SIGBUS, &action, NULL) == 0);
    if (!strcmp(argv[1], "exec-child")) { assert(argc == 5); exec_child(argv); return 0; }
    if (!strcmp(argv[1], "geometry")) geometry();
    else if (!strcmp(argv[1], "protection")) protection();
    else if (!strcmp(argv[1], "holes")) holes();
    else if (!strcmp(argv[1], "owners")) owners();
    else if (!strcmp(argv[1], "file-offsets")) file_offsets();
    else if (!strcmp(argv[1], "truncate")) truncate_faults();
    else if (!strcmp(argv[1], "discard-remap")) discard_remap();
    else if (!strcmp(argv[1], "cow")) cow();
    else if (!strcmp(argv[1], "threads")) threads();
    else if (!strcmp(argv[1], "retire")) retire();
    else if (!strcmp(argv[1], "exec")) exec_state(argv[0]);
    else abort();
    printf("%s: ok\n", argv[1]); return 0;
}
