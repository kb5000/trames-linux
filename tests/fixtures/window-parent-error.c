// SPDX-License-Identifier: GPL-3.0-or-later
// The original scanner header supplies the independent protocol error oracle.
#define main original_window_main
#include "window-client.c"
#undef main
_Static_assert(XDG_TOPLEVEL_ERROR_INVALID_PARENT==1,"original xdg-shell invalid_parent wire value");
int main(void){setbuf(stdout,NULL);display=wl_display_connect(NULL);assert(display);struct wl_registry*r=wl_display_get_registry(display);wl_registry_add_listener(r,&rl,NULL);assert(wl_display_roundtrip(display)>=0&&compositor&&shm&&shell);create(&parent,"parent");assert(wl_display_roundtrip(display)>=0);create(&child,"child");assert(wl_display_roundtrip(display)>=0);assert(wl_display_roundtrip(display)>=0);xdg_toplevel_set_parent(parent.top,child.top);assert(wl_display_roundtrip(display)<0);const struct wl_interface*interface=NULL;uint32_t id=0;uint32_t error=wl_display_get_protocol_error(display,&interface,&id);printf("parent-error code=%u interface=%s id=%u\n",error,interface?interface->name:"none",id);assert(interface&&!strcmp(interface->name,"xdg_toplevel")&&error==XDG_TOPLEVEL_ERROR_INVALID_PARENT);wl_display_disconnect(display);puts("parent-error-done");return 0;}
