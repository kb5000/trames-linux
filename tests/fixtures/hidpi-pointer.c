// SPDX-License-Identifier: GPL-3.0-or-later
#include <wayland-client.h>
#include "wlr-virtual-pointer-unstable-v1-client-protocol.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
static struct zwlr_virtual_pointer_manager_v1 *manager;
static void global(void*d,struct wl_registry*r,uint32_t id,const char*n,uint32_t v){(void)d;(void)v;if(!strcmp(n,"zwlr_virtual_pointer_manager_v1"))manager=wl_registry_bind(r,id,&zwlr_virtual_pointer_manager_v1_interface,1);}
static void removed(void*d,struct wl_registry*r,uint32_t id){(void)d;(void)r;(void)id;}
static const struct wl_registry_listener listener={global,removed};
int main(void){struct wl_display*d=wl_display_connect(NULL);assert(d);struct wl_registry*r=wl_display_get_registry(d);wl_registry_add_listener(r,&listener,NULL);assert(wl_display_roundtrip(d)>=0&&manager);struct zwlr_virtual_pointer_v1*p=zwlr_virtual_pointer_manager_v1_create_virtual_pointer(manager,NULL);assert(wl_display_roundtrip(d)>=0);puts("pointer-ready");fflush(stdout);char c;while(read(0,&c,1)>0){}zwlr_virtual_pointer_v1_destroy(p);zwlr_virtual_pointer_manager_v1_destroy(manager);wl_display_roundtrip(d);wl_display_disconnect(d);return 0;}
