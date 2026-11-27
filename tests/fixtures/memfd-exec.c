// SPDX-License-Identifier: GPL-3.0-or-later
// Original Linux self-reexec fixture; identical executable bytes, no SDK or instruction patching.
#define _GNU_SOURCE
#include <sys/mman.h>
#include <sys/sendfile.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
extern char **environ;
static void fail(const char*s){perror(s);exit(1);}
int main(int argc,char**argv){
 if(argc==2){int fd=open("/proc/self/exe",O_RDONLY|O_CLOEXEC);if(fd<0)fail("reopen memfd exe");char h[4];if(read(fd,h,4)!=4||memcmp(h,"\177ELF",4))fail("ELF bytes");if(fcntl(fd,F_GET_SEALS)!=15)fail("seals");puts("MEMFD_EXEC_OK");return 0;}
 int src=open("/proc/self/exe",O_RDONLY|O_CLOEXEC);if(src<0)fail("open self");struct stat st;if(fstat(src,&st))fail("stat");int out=memfd_create("self-clone",MFD_CLOEXEC|MFD_ALLOW_SEALING);if(out<0)fail("memfd");off_t at=0;while(at<st.st_size){if(sendfile(out,src,&at,(size_t)(st.st_size-at))<=0)fail("copy");}close(src);if(fcntl(out,F_ADD_SEALS,15))fail("seal");char*args[]={argv[0],"child",NULL};execveat(out,"",args,environ,AT_EMPTY_PATH);fail("execveat");}
