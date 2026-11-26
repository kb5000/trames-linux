// SPDX-License-Identifier: GPL-3.0-or-later
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <pthread.h>
#include <setjmp.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <unistd.h>

static _Thread_local unsigned thread_value=73;
static atomic_uint counter;
static pthread_barrier_t barrier;
static sigjmp_buf recovery;
static volatile sig_atomic_t trapped;
static void *volatile bad_address;
static void segv(int signal,siginfo_t *info,void *context) {
    (void)context;
    if (signal!=SIGSEGV || info->si_addr!=bad_address) _exit(91);
    trapped++;
    siglongjmp(recovery,1);
}
static void *thread(void *argument) {
    unsigned value=(unsigned)(uintptr_t)argument;
    assert(thread_value==73);thread_value=value;
    pthread_barrier_wait(&barrier);
    for(unsigned i=0;i<5000;i++) atomic_fetch_add_explicit(&counter,1,memory_order_relaxed);
    assert(thread_value==value);return (void *)(uintptr_t)value;
}
int main(int argc,char **argv) {
    assert(argc==2);
    if(!strcmp(argv[1],"spin")) for(;;) __asm__ volatile("":::"memory");
    if(!strcmp(argv[1],"sleep")) {sleep(30);return 92;}
    if(!strcmp(argv[1],"exit")) return 37;
    if(!strcmp(argv[1],"basic")) {
        assert(sysconf(_SC_PAGESIZE)==4096);assert(getpid()>0);assert(getppid()>=0);assert(thread_value==73);
        puts("basic: ok");return 0;
    }
    if(!strcmp(argv[1],"memory")) {
        unsigned char *p=mmap(NULL,3*4096,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS,-1,0);assert(p!=MAP_FAILED);
        p[0]=11;p[4096]=22;p[8192]=33;
        struct sigaction action={.sa_sigaction=segv,.sa_flags=SA_SIGINFO};sigemptyset(&action.sa_mask);assert(!sigaction(SIGSEGV,&action,NULL));
        assert(!mprotect(p+4096,4096,PROT_READ));bad_address=p+4096;
        if(!sigsetjmp(recovery,1)) *(volatile unsigned char *)(p+4096)=99;
        assert(trapped==1 && p[4096]==22 && p[0]==11 && p[8192]==33);
        assert(!mprotect(p+4096,4096,PROT_READ|PROT_WRITE));p[4096]=44;
        assert(!munmap(p+8192,4096));bad_address=p+8192;
        if(!sigsetjmp(recovery,1)) (void)*(volatile unsigned char *)(p+8192);
        assert(trapped==2 && p[4096]==44);assert(!munmap(p,8192));
        puts("memory: ok");return 0;
    }
    if(!strcmp(argv[1],"fork")) {
        unsigned *p=mmap(NULL,4096,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS,-1,0);assert(p!=MAP_FAILED);*p=123;
        int pipefd[2];assert(!pipe(pipefd));pid_t child=fork();assert(child>=0);
        if(!child){close(pipefd[0]);*p=456;assert(write(pipefd[1],"child",5)==5);_exit(17);}
        close(pipefd[1]);char text[5];assert(read(pipefd[0],text,5)==5 && !memcmp(text,"child",5));
        int status;assert(waitpid(child,&status,0)==child && WIFEXITED(status) && WEXITSTATUS(status)==17);assert(*p==123);
        assert(!munmap(p,4096));close(pipefd[0]);puts("fork: ok");return 0;
    }
    if(!strcmp(argv[1],"threads")) {
        pthread_t workers[8];assert(!pthread_barrier_init(&barrier,NULL,9));
        for(unsigned i=0;i<8;i++) assert(!pthread_create(&workers[i],NULL,thread,(void *)(uintptr_t)(100+i)));
        pthread_barrier_wait(&barrier);
        for(unsigned i=0;i<8;i++){void *result;assert(!pthread_join(workers[i],&result));assert((uintptr_t)result==100+i);}
        assert(atomic_load(&counter)==40000 && thread_value==73);pthread_barrier_destroy(&barrier);
        puts("threads: ok");return 0;
    }
    return 90;
}
