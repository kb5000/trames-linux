// SPDX-License-Identifier: GPL-3.0-or-later
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/ioctl.h>
int main(int argc,char **argv) {
    if(argc<2)return 80;
    if(!strcmp(argv[1],"stdio")) {
        char data[4096];ssize_t n;while((n=read(0,data,sizeof data))>0){ssize_t at=0;while(at<n){ssize_t out=write(1,data+at,n-at);if(out<=0)return 81;at+=out;}}
        if(n<0)return 82;if(write(2,"guest-stderr\n",13)!=13)return 87;return 7;
    }
    if(!strcmp(argv[1],"env")) {char cwd[256];if(!getcwd(cwd,sizeof cwd))return 83;printf("%s:%s\n",cwd,getenv("CLI_VALUE"));return 0;}
    if(!strcmp(argv[1],"tty")) {struct winsize size;if(!isatty(0)||ioctl(0,TIOCGWINSZ,&size))return 84;printf("tty:%u,%u\n",size.ws_row,size.ws_col);fflush(stdout);char line[128];if(!fgets(line,sizeof line,stdin))return 85;printf("line:%s",line);return 0;}
    if(!strcmp(argv[1],"spin")) {for(;;)__asm__ volatile("":::"memory");}
    if(!strcmp(argv[1],"sleep")) {sleep(30);return 0;}
    return 86;
}
