// SPDX-License-Identifier: GPL-3.0-or-later
#define _GNU_SOURCE
#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
int main(int argc,char **argv) {
    assert(argc==2);
    if(!strcmp(argv[1],"file")) {
        int w=open("/work/cache",O_CREAT|O_TRUNC|O_WRONLY,0600);assert(w>=0);
        int r=open("/work/cache",O_RDONLY);assert(r>=0);
        for(unsigned i=0;i<128;i++) {
            unsigned char input[32],output[32];memset(input,(unsigned char)i,sizeof input);
            assert(pwrite(w,input,sizeof input,0)==sizeof input);
            assert(pread(r,output,sizeof output,0)==sizeof output);
            assert(!memcmp(input,output,sizeof input));
        }
        assert(!rename("/work/cache","/work/renamed"));assert(!unlink("/work/renamed"));
        unsigned char output[32];assert(pread(r,output,sizeof output,0)==sizeof output);
        for(unsigned i=0;i<sizeof output;i++)assert(output[i]==127);
        close(w);close(r);puts("file-cache: ok");return 0;
    }
    if(!strcmp(argv[1],"jit")) {
#if defined(__aarch64__)
        for(unsigned n=1;n<=64;n++) {
            uint32_t *p=mmap(NULL,4096,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS,-1,0);assert(p!=MAP_FAILED);
            // Original guest-generated application code, never rewritten by Trames.
            p[0]=0x52800000|(n<<5);p[1]=0xd65f03c0; // mov w0,#n; ret
            __builtin___clear_cache((char*)p,(char*)(p+2));
            assert(!mprotect(p,4096,PROT_READ|PROT_EXEC));
            assert(((int(*)(void))p)()==(int)n);assert(!munmap(p,4096));
        }
        puts("jit-cache: ok");return 0;
#else
        return 77;
#endif
    }
    return 2;
}
