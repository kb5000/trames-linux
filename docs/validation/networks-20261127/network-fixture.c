// SPDX-License-Identifier: GPL-3.0-or-later
// Original POSIX socket fixture; no Trames SDK, injected syscall or test hook.
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <unistd.h>
static void fail(const char *s){perror(s);exit(1);}
static void send_all(int f,const void *p,size_t n){while(n){ssize_t r=write(f,p,n);if(r<0&&errno==EINTR)continue;if(r<=0)fail("write");p=(const char*)p+r;n-=(size_t)r;}}
static int endpoint(const char *ip,const char *port,int tcp,int server){
 struct sockaddr_storage ss={0};socklen_t size;int family=strchr(ip,':')?AF_INET6:AF_INET;
 if(family==AF_INET){struct sockaddr_in*a=(void*)&ss;a->sin_family=family;a->sin_port=htons((unsigned short)atoi(port));if(inet_pton(family,ip,&a->sin_addr)!=1)exit(2);size=sizeof(*a);}
 else{struct sockaddr_in6*a=(void*)&ss;a->sin6_family=family;a->sin6_port=htons((unsigned short)atoi(port));if(inet_pton(family,ip,&a->sin6_addr)!=1)exit(2);size=sizeof(*a);}
 int f=socket(family,tcp?SOCK_STREAM:SOCK_DGRAM,0);if(f<0)fail("socket");
 struct timeval timeout={server?30:3,0};if(setsockopt(f,SOL_SOCKET,SO_RCVTIMEO,&timeout,sizeof(timeout))||setsockopt(f,SOL_SOCKET,SO_SNDTIMEO,&timeout,sizeof(timeout)))fail("timeout");
 if(server){int one=1;if(tcp&&setsockopt(f,SOL_SOCKET,SO_REUSEADDR,&one,sizeof(one)))fail("reuse");if(bind(f,(void*)&ss,size))fail("bind");if(tcp&&listen(f,16))fail("listen");}
 else if(connect(f,(void*)&ss,size))fail("connect");
 return f;
}
int main(int argc,char**argv){
 setbuf(stdout,NULL);
 if(argc==3&&!strcmp(argv[1],"resolve")){struct addrinfo hint={.ai_family=AF_INET,.ai_socktype=SOCK_STREAM},*result=NULL;int code=getaddrinfo(argv[2],NULL,&hint,&result);if(code){fprintf(stderr,"resolve: %s\n",gai_strerror(code));return 1;}char ip[INET_ADDRSTRLEN];puts(inet_ntop(AF_INET,&((struct sockaddr_in*)result->ai_addr)->sin_addr,ip,sizeof(ip)));freeaddrinfo(result);return 0;}
 if(argc<5)return 2;
 int tcp=!strcmp(argv[2],"tcp"),server=!strcmp(argv[1],"server");if(!tcp&&strcmp(argv[2],"udp"))return 2;
 int f=endpoint(argv[3],argv[4],tcp,server);unsigned char buffer[65536];
 if(server){int count=argc>5?atoi(argv[5]):1;puts("READY");for(int i=0;i<count;i++){
  if(tcp){int c=accept(f,NULL,NULL);if(c<0)fail("accept");for(;;){ssize_t n=read(c,buffer,sizeof(buffer));if(n<0&&errno==EINTR)continue;if(n<0)fail("read");if(!n)break;send_all(c,buffer,(size_t)n);}close(c);}
  else{struct sockaddr_storage peer;socklen_t size=sizeof(peer);ssize_t n=recvfrom(f,buffer,sizeof(buffer),0,(void*)&peer,&size);if(n<0)fail("recvfrom");if(sendto(f,buffer,(size_t)n,0,(void*)&peer,size)!=n)fail("sendto");}
 }close(f);return 0;}
 size_t used=0,capacity=4*1024*1024;unsigned char*body=malloc(capacity);if(!body)fail("malloc");for(;;){ssize_t n=read(0,body+used,capacity-used);if(n<0&&errno==EINTR)continue;if(n<0)fail("stdin");if(!n)break;used+=(size_t)n;if(used==capacity)return 2;}
 if(tcp){send_all(f,body,used);if(shutdown(f,SHUT_WR))fail("shutdown");for(;;){ssize_t n=read(f,buffer,sizeof(buffer));if(n<0&&errno==EINTR)continue;if(n<0)fail("reply");if(!n)break;send_all(1,buffer,(size_t)n);}}
 else{if(send(f,body,used,0)!=(ssize_t)used)fail("send");ssize_t n=recv(f,buffer,sizeof(buffer),0);if(n<0)fail("recv");send_all(1,buffer,(size_t)n);}
 free(body);close(f);return 0;
}
