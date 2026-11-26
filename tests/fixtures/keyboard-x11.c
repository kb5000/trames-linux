// SPDX-License-Identifier: GPL-3.0-or-later
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/XKBlib.h>
#include <assert.h>
#include <locale.h>
#include <stdio.h>
#include <unistd.h>
#include <poll.h>
static void map(Display*d){printf("keymap q=%lx y=%lx\n",XkbKeycodeToKeysym(d,24,0,0),XkbKeycodeToKeysym(d,29,0,0));}
int main(void){setbuf(stdout,NULL);assert(setlocale(LC_ALL,""));XSetLocaleModifiers("@im=none");Display*d=XOpenDisplay(NULL);assert(d);Window w=XCreateSimpleWindow(d,DefaultRootWindow(d),0,0,240,160,0,0,0x20c040);XStoreName(d,w,"Trames desktop acceptance");XSelectInput(d,w,KeyPressMask|KeyReleaseMask|FocusChangeMask|ExposureMask|StructureNotifyMask);XMapWindow(d,w);XIM im=XOpenIM(d,NULL,NULL,NULL);assert(im);XIC ic=XCreateIC(im,XNInputStyle,XIMPreeditNothing|XIMStatusNothing,XNClientWindow,w,XNFocusWindow,w,NULL);assert(ic);map(d);puts("x11-keyboard-ready");
 for(;;){while(XPending(d)){XEvent e;XNextEvent(d,&e);if(XFilterEvent(&e,None))continue;if(e.type==MappingNotify){XRefreshKeyboardMapping(&e.xmapping);map(d);}if(e.type==FocusIn){XSetICFocus(ic);puts("focus-in");}if(e.type==FocusOut){XUnsetICFocus(ic);puts("focus-out");}if(e.type==KeyPress){char text[128]={0};KeySym sym;Status status;int n=Xutf8LookupString(ic,&e.xkey,text,sizeof(text)-1,&sym,&status);assert(n>=0&&n<(int)sizeof(text));text[n]=0;printf("physical %u 1 sym=%lx text=%s\n",e.xkey.keycode-8,sym,text);}if(e.type==KeyRelease)printf("physical %u 0\n",e.xkey.keycode-8);}
 XFlush(d);if(XEventsQueued(d,QueuedAlready))continue;struct pollfd fds[2]={{ConnectionNumber(d),POLLIN,0},{0,POLLIN,0}};assert(poll(fds,2,30000)>0);if(fds[1].revents&POLLIN){char q;assert(read(0,&q,1)==1&&q=='q');break;}}
 XDestroyIC(ic);XCloseIM(im);XDestroyWindow(d,w);XCloseDisplay(d);puts("keyboard-done");return 0;}
