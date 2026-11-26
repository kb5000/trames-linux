// SPDX-License-Identifier: GPL-3.0-or-later
#include <X11/Xlib.h>
#include <X11/XKBlib.h>
#include <X11/Xmd.h>
#include <X11/extensions/XKBproto.h>
#include <assert.h>
#include <stddef.h>
#include <stdlib.h>
// Compile-time checks against the distribution's original XKB wire header.
_Static_assert(sizeof(xkbLatchLockStateReq)==16,"XKB-v1 request size");
_Static_assert(offsetof(xkbLatchLockStateReq,modLatches)==11,"XKB modLatches byte");
_Static_assert(offsetof(xkbLatchLockStateReq,groupLock)==9,"XKB groupLock byte");
_Static_assert(offsetof(xkbLatchLockStateReq,groupLatch)==14,"XKB groupLatch bytes");
int main(int argc,char**argv){assert(argc==2);Display*d=XOpenDisplay(NULL);assert(d);assert(XkbLatchModifiers(d,XkbUseCoreKbd,255,(unsigned)strtoul(argv[1],NULL,0)));XSync(d,False);XCloseDisplay(d);return 0;}
