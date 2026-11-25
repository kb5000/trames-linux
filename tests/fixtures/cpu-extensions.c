// SPDX-License-Identifier: GPL-3.0-or-later
// Original native/KVM ELF: advertised instructions, FP state, signals and ptrace.
#define _GNU_SOURCE
#include <assert.h>
#include <ctype.h>
#include <asm/ptrace.h>
#include <elf.h>
#include <pthread.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>
#include <sys/ptrace.h>
#include <sys/syscall.h>
#include <sys/uio.h>
#include <sys/wait.h>
#include <ucontext.h>
#include <unistd.h>

static void vector_result(const char* name, const unsigned char* bytes, uint64_t fpsr) {
    printf("%s:",name);
    for(unsigned i=0;i<16;i++)printf("%02x",bytes[i]);
    printf(" fpsr=%08lx\n",fpsr);
}
#define VECTOR(word,bit,name,instructions) do { if (selected[word]&(UINT64_C(1)<<bit)) { \
    unsigned char out[16] __attribute__((aligned(16)));uint64_t status; \
    __asm__ volatile("msr fpcr,xzr\nmsr fpsr,xzr\nmovi v0.16b,#0x11\nmovi v1.16b,#0x22\nmovi v2.16b,#0x33\nmovi v3.16b,#0x44\n" instructions "\nstr q0,[%1]\nmrs %0,fpsr" : "=r"(status) : "r"(out) : "v0","v1","v2","v3","x9","memory","cc"); \
    vector_result(name,out,status);covered[word]|=UINT64_C(1)<<bit; \
} } while(0)

static void instructions(uint64_t low,uint64_t high,const char* expected_names) {
    uint64_t selected[]={low,high},covered[]={3,0};
    assert((getauxval(AT_HWCAP)&low)==low && (getauxval(AT_HWCAP2)&high)==high);
    assert(getauxval(AT_PAGESZ)==4096);
    if(expected_names) {
        assert(getauxval(AT_HWCAP)==low && getauxval(AT_HWCAP2)==high);
        FILE* file=fopen("/proc/cpuinfo","r");assert(file);char line[2048];int found=0;
        while(fgets(line,sizeof(line),file))if(!strncmp(line,"Features",8)) {
            char* value=strchr(line,':');assert(value);value++;while(isspace((unsigned char)*value))value++;
            value[strcspn(value,"\r\n")]=0;assert(!strcmp(value,expected_names));found=1;break;
        }
        fclose(file);assert(found);
    }
    printf("profile: %016lx %016lx\n",low,high);
    VECTOR(0,3,"aes","aese v0.16b,v1.16b\naesmc v0.16b,v0.16b");
    VECTOR(0,4,"pmull","pmull v0.1q,v1.1d,v2.1d");
    VECTOR(0,5,"sha1","sha1h s0,s1");
    VECTOR(0,6,"sha2","sha256h q0,q1,v2.4s");
    if(low&(1<<7)) {uint32_t value=0x12345678;uint64_t source=0xabcdef0123456789;__asm__ volatile("crc32x %w0,%w0,%1\ncrc32cx %w0,%w0,%1":"+r"(value):"r"(source));printf("crc32: %08x\n",value);covered[0]|=1<<7;}
    if(low&(1<<8)) {uint64_t value=7,old;__asm__ volatile("ldaddal %2,%0,[%1]":"=&r"(old):"r"(&value),"r"(UINT64_C(3)):"memory");assert(old==7&&value==10);puts("atomics: ok");covered[0]|=1<<8;}
    VECTOR(0,9,"fphp","fadd h0,h1,h2");
    VECTOR(0,10,"asimdhp","fadd v0.8h,v1.8h,v2.8h");
    VECTOR(0,12,"asimdrdm","sqrdmlah v0.4s,v1.4s,v2.4s");
    VECTOR(0,13,"jscvt","fjcvtzs w9,d1\nfmov s0,w9");
    VECTOR(0,14,"fcma","fcmla v0.4s,v1.4s,v2.4s,#90");
    if(low&(1<<15)) {uint64_t source=0x123456789abcdef0,value;__asm__ volatile("ldapr %0,[%1]":"=r"(value):"r"(&source):"memory");assert(value==source);puts("lrcpc: ok");covered[0]|=1<<15;}
    unsigned char cache[64] __attribute__((aligned(64)));memset(cache,0x5a,sizeof(cache));
    if(low&(1<<16)) {__asm__ volatile("dc cvap,%0\ndsb ish"::"r"(cache):"memory");assert(cache[0]==0x5a);puts("dcpop: ok");covered[0]|=1<<16;}
    VECTOR(0,17,"sha3","eor3 v0.16b,v0.16b,v1.16b,v2.16b");
    VECTOR(0,18,"sm3","sm3ss1 v0.4s,v1.4s,v2.4s,v3.4s");
    VECTOR(0,19,"sm4","sm4e v0.4s,v1.4s");
    VECTOR(0,20,"asimddp","sdot v0.4s,v1.16b,v2.16b");
    VECTOR(0,21,"sha512","sha512h q0,q1,v2.2d");
    VECTOR(0,23,"asimdfhm","fmlal v0.4s,v1.4h,v2.4h");
    if(low&(1<<25)) {uint64_t source=0x123456789abcdef0,value;unsigned char bytes[32] __attribute__((aligned(16)));memcpy(bytes+1,&source,8);__asm__ volatile("ldr %0,[%1]":"=r"(value):"r"(bytes+1):"memory");assert(value==source);puts("uscat: unaligned access ok");covered[0]|=1<<25;}
    if(low&(1<<26)) {uint64_t source=0x123456789abcdef0,value;__asm__ volatile("ldapur %0,[%1]":"=r"(value):"r"(&source):"memory");assert(value==source);puts("ilrcpc: ok");covered[0]|=1<<26;}
    if(low&(1<<27)) {uint64_t value=0x60000000;__asm__ volatile("msr nzcv,%0\ncfinv\nmrs %0,nzcv":"+r"(value)::"cc");assert(value==0x40000000);puts("flagm: ok");covered[0]|=1<<27;}
    if(low&(1<<29)) {__asm__ volatile("sb":::"memory");puts("sb: ok");covered[0]|=1<<29;}
    if(high&1) {__asm__ volatile("dc cvadp,%0\ndsb ish"::"r"(cache):"memory");assert(cache[0]==0x5a);puts("dcpodp: ok");covered[1]|=1;}
    if(high&(1<<7)) {uint64_t value=0x60000000;__asm__ volatile("msr nzcv,%0\naxflag\nmrs %0,nzcv":"+r"(value)::"cc");printf("flagm2: %08lx\n",value);covered[1]|=1<<7;}
    VECTOR(1,8,"frint","frint32z d0,d1");
    VECTOR(1,13,"i8mm","smmla v0.4s,v1.16b,v2.16b");
    VECTOR(1,14,"bf16","bfdot v0.4s,v1.8h,v2.8h");
    if(high&(1<<15)) {__asm__ volatile("hint #6":::"memory");puts("dgh: ok");covered[1]|=1<<15;}
    if(high&(1<<16)) {uint64_t value;__asm__ volatile("mrs %0,S3_3_C2_C4_0":"=r"(value)::"cc");puts("rng: instruction completed");covered[1]|=1<<16;}
    if(high&(1<<20)) {uint64_t control=7,read;__asm__ volatile("msr fpcr,%1\nmrs %0,fpcr\nmsr fpcr,xzr":"=r"(read):"r"(control));assert((read&7)==7);puts("afp: control ok");covered[1]|=1<<20;}
    VECTOR(1,21,"rpres","frecpe v0.4s,v1.4s");
    VECTOR(1,32,"ebf16","mov x9,#0x2000\nmsr fpcr,x9\nbfdot v0.4s,v1.8h,v2.8h\nmsr fpcr,xzr");
    // LUTI4 v0.16b,{v1.16b},v2[0], assembled independently by LLVM 23.
    // The ELF contains the original instruction; the runtime never patches it.
    VECTOR(1,49,"lut",".inst 0x4e422020");
    assert((selected[0]&~covered[0])==0 && (selected[1]&~covered[1])==0);
    puts("instructions: ok");
}

static _Thread_local uint32_t expected_fpcr,expected_fpsr;
static _Thread_local volatile sig_atomic_t signal_count,signal_errors;
static void state_signal(int signal,siginfo_t* info,void* context) {
    (void)signal;(void)info;
    ucontext_t* u=context;unsigned char* p=u->uc_mcontext.__reserved;
    for(size_t at=0;at+8<=sizeof(u->uc_mcontext.__reserved);) {
        uint32_t magic,size;memcpy(&magic,p+at,4);memcpy(&size,p+at+4,4);
        if(!magic)break;
        if(size<8||size%16||at+size>sizeof(u->uc_mcontext.__reserved))break;
        if(magic==0x46508001 && size>=528) {
            uint32_t fpsr,fpcr;memcpy(&fpsr,p+at+8,4);memcpy(&fpcr,p+at+12,4);
            if(fpsr!=expected_fpsr||fpcr!=expected_fpcr)signal_errors++;
            signal_count++;return;
        }
        at+=size;
    }
    signal_errors++;
}
static long raw(long n,long a,long b,long c) {
    register long x0 __asm__("x0")=a;register long x1 __asm__("x1")=b;register long x2 __asm__("x2")=c;register long x8 __asm__("x8")=n;
    __asm__ volatile("svc #0":"+r"(x0):"r"(x1),"r"(x2),"r"(x8):"memory","cc");return x0;
}
static void* state_thread(void* argument) {
    unsigned i=(uintptr_t)argument;long pid=getpid(),tid=raw(SYS_gettid,0,0,0);
    uint64_t hwcap2=getauxval(AT_HWCAP2);
    expected_fpcr=0x03000000|(i<<22);
    if(hwcap2&(UINT64_C(1)<<20))expected_fpcr|=(i*3)&7; // AFP: FIZ/AH/NEP
    if(hwcap2&(UINT64_C(1)<<32))expected_fpcr|=1<<13;   // EBF16
    expected_fpsr=0x08000080|(i+1);
    __asm__ volatile("msr fpcr,%0\nmsr fpsr,%1"::"r"((uint64_t)expected_fpcr),"r"((uint64_t)expected_fpsr));
    for(unsigned turn=0;turn<128;turn++) {
        uint64_t counter=100000;
        __asm__ volatile("1: subs %0,%0,#1\nb.ne 1b":"+r"(counter)::"cc");
        assert(raw(SYS_getpid,0,0,0)==pid);
        if(turn%2==0)assert(raw(SYS_tgkill,pid,tid,SIGUSR1)==0);
        assert(raw(SYS_sched_yield,0,0,0)==0);
        uint64_t fpcr,fpsr;__asm__ volatile("mrs %0,fpcr\nmrs %1,fpsr":"=r"(fpcr),"=r"(fpsr));
        assert(fpcr==expected_fpcr && fpsr==expected_fpsr);
    }
    assert(signal_count==64 && signal_errors==0);
    __asm__ volatile("msr fpcr,xzr\nmsr fpsr,xzr");return NULL;
}
static void state(void) {
    struct sigaction action={.sa_sigaction=state_signal,.sa_flags=SA_SIGINFO};sigemptyset(&action.sa_mask);assert(sigaction(SIGUSR1,&action,NULL)==0);
    pthread_t threads[4];for(uintptr_t i=0;i<4;i++)assert(pthread_create(&threads[i],NULL,state_thread,(void*)i)==0);
    for(unsigned i=0;i<4;i++)assert(pthread_join(threads[i],NULL)==0);
    puts("state: 4 threads, 256 signal frames, nonzero FPCR/FPSR ok");
}

__asm__(".pushsection .text\n.balign 4\n.global traced_instructions\ntraced_instructions:\nadd x0,x0,#1\nadd x0,x0,#2\nsvc #0\nadd x0,x0,#3\nmov x0,#0\nmov x8,#93\nsvc #0\n.popsection");
extern void traced_instructions(void);
static void tracing(void) {
    pid_t child=fork();assert(child>=0);
    if(!child) {assert(ptrace(PTRACE_TRACEME,0,NULL,NULL)==0);raise(SIGSTOP);_exit(99);}
    int status;assert(waitpid(child,&status,0)==child && WIFSTOPPED(status));
    struct user_pt_regs registers;struct iovec io={.iov_base=&registers,.iov_len=sizeof(registers)};
    assert(ptrace(PTRACE_GETREGSET,child,(void*)NT_PRSTATUS,&io)==0);
    registers.pc=(uintptr_t)traced_instructions;registers.regs[0]=10;registers.regs[8]=SYS_getpid;
    io.iov_len=sizeof(registers);assert(ptrace(PTRACE_SETREGSET,child,(void*)NT_PRSTATUS,&io)==0);
    const unsigned offsets[]={4,8,12,12,16};
    for(unsigned i=0;i<5;i++) {
        assert(ptrace(PTRACE_SINGLESTEP,child,NULL,NULL)==0);
        assert(waitpid(child,&status,0)==child && WIFSTOPPED(status) && WSTOPSIG(status)==SIGTRAP);
        siginfo_t info;assert(ptrace(PTRACE_GETSIGINFO,child,NULL,&info)==0);
        // Linux arm64's syscall-completion step reports plain SIGTRAP (code 0);
        // architectural instruction steps report TRAP_TRACE. Verify each path.
        int expected_code=i==2?0:TRAP_TRACE;
        if(info.si_code!=expected_code)fprintf(stderr,"single-step %u: si_code=%d expected=%d\n",i,info.si_code,expected_code);
        assert(info.si_code==expected_code);
        io.iov_len=sizeof(registers);assert(ptrace(PTRACE_GETREGSET,child,(void*)NT_PRSTATUS,&io)==0);
        assert(registers.pc==(uintptr_t)traced_instructions+offsets[i]);
        assert(registers.regs[0]==(i==0?11:i==1?13:i<=3?(uint64_t)child:(uint64_t)child+3));
    }
    assert(ptrace(PTRACE_CONT,child,NULL,NULL)==0);
    assert(waitpid(child,&status,0)==child && WIFEXITED(status) && WEXITSTATUS(status)==0);
    puts("ptrace: original instructions and syscall single-step ok");
}
int main(int argc,char** argv) {
    setbuf(stdout,NULL);assert(argc>=2);
    if(!strcmp(argv[1],"instructions")) {assert(argc==4||argc==5);instructions(strtoull(argv[2],NULL,0),strtoull(argv[3],NULL,0),argc==5?argv[4]:NULL);}
    else if(!strcmp(argv[1],"state"))state();
    else if(!strcmp(argv[1],"ptrace"))tracing();
    else abort();
    return 0;
}
