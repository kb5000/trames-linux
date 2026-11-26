// SPDX-License-Identifier: GPL-3.0-or-later
// Original Xlib/XIM client. Standard instantiate callbacks handle an input
// method that becomes available after the Xwayland socket starts listening.
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <assert.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
static Window window;static XIM method;static XIC context;
static void available(Display*d,XPointer data,XPointer call){(void)data;(void)call;if(method)return;method=XOpenIM(d,NULL,NULL,NULL);if(!method)return;context=XCreateIC(method,XNInputStyle,XIMPreeditNothing|XIMStatusNothing,XNClientWindow,window,XNFocusWindow,window,NULL);assert(context);XSetICFocus(context);puts("guest-xim-ready");}
int main(void){setbuf(stdout,NULL);assert(setlocale(LC_ALL,""));XSetLocaleModifiers("");Display*d=XOpenDisplay(NULL);assert(d);int screen=DefaultScreen(d);window=XCreateSimpleWindow(d,RootWindow(d,screen),0,0,240,160,0,0,0x20c040);XStoreName(d,window,"Trames XIM acceptance");XSelectInput(d,window,KeyPressMask|FocusChangeMask|ExposureMask|StructureNotifyMask);XMapWindow(d,window);XFlush(d);available(d,NULL,NULL);if(!method)assert(XRegisterIMInstantiateCallback(d,NULL,NULL,NULL,available,NULL));
 for(;;){XEvent e;XNextEvent(d,&e);if(XFilterEvent(&e,None))continue;if(!context)continue;if(e.type==FocusIn)XSetICFocus(context);if(e.type==FocusOut)XUnsetICFocus(context);if(e.type==KeyPress){char bytes[4096];KeySym sym;Status status;int n=Xutf8LookupString(context,&e.xkey,bytes,sizeof(bytes)-1,&sym,&status);assert(n>=0&&n<(int)sizeof(bytes));if(n){bytes[n]=0;printf("guest-xim-commit %s\n",bytes);if(n>1){XDestroyIC(context);XCloseIM(method);XDestroyWindow(d,window);XCloseDisplay(d);return 0;}}}}
}
