// SPDX-License-Identifier: GPL-3.0-or-later
// Native-only test capability; the product never loads this helper.
#include <assert.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <wayland-client.h>
#include "xdg-shell-client-protocol.h"
struct target { struct wl_display* display;struct wl_compositor* compositor;struct xdg_wm_base* shell;struct wl_surface* surface;struct xdg_surface* xdg;struct xdg_toplevel* top;int configured; };
static void ping(void* data,struct xdg_wm_base* shell,uint32_t serial){(void)data;xdg_wm_base_pong(shell,serial);}
static const struct xdg_wm_base_listener shell_listener={ping};
static void global(void* data,struct wl_registry* registry,uint32_t name,const char* interface,uint32_t version) {
    struct target* t=data;
    if(!strcmp(interface,"wl_compositor"))t->compositor=wl_registry_bind(registry,name,&wl_compositor_interface,version<4?version:4);
    if(!strcmp(interface,"xdg_wm_base")){t->shell=wl_registry_bind(registry,name,&xdg_wm_base_interface,1);xdg_wm_base_add_listener(t->shell,&shell_listener,t);}
}
static void removed(void* data,struct wl_registry* registry,uint32_t name){(void)data;(void)registry;(void)name;}
static const struct wl_registry_listener registry_listener={global,removed};
static void configure(void* data,struct xdg_surface* surface,uint32_t serial){xdg_surface_ack_configure(surface,serial);((struct target*)data)->configured=1;}
static const struct xdg_surface_listener surface_listener={configure};
static void size(void* data,struct xdg_toplevel* top,int32_t width,int32_t height,struct wl_array* states){(void)data;(void)top;(void)width;(void)height;(void)states;}
static void close_window(void* data,struct xdg_toplevel* top){(void)data;(void)top;}
static const struct xdg_toplevel_listener top_listener={.configure=size,.close=close_window};
void* test_target_new(void) {
    struct target* t=calloc(1,sizeof(*t));assert(t);t->display=wl_display_connect(NULL);assert(t->display);
    struct wl_registry* registry=wl_display_get_registry(t->display);wl_registry_add_listener(registry,&registry_listener,t);assert(wl_display_roundtrip(t->display)>=0);assert(t->compositor&&t->shell);
    t->surface=wl_compositor_create_surface(t->compositor);t->xdg=xdg_wm_base_get_xdg_surface(t->shell,t->surface);xdg_surface_add_listener(t->xdg,&surface_listener,t);
    t->top=xdg_surface_get_toplevel(t->xdg);xdg_toplevel_add_listener(t->top,&top_listener,t);xdg_toplevel_set_title(t->top,"Trames native Wayland target");wl_surface_commit(t->surface);
    while(!t->configured)assert(wl_display_dispatch(t->display)>=0);
    wl_registry_destroy(registry);return t;
}
void* test_target_display(void* value){return ((struct target*)value)->display;}
void* test_target_surface(void* value){return ((struct target*)value)->surface;}
void test_target_free(void* value){struct target* t=value;xdg_toplevel_destroy(t->top);xdg_surface_destroy(t->xdg);wl_surface_destroy(t->surface);xdg_wm_base_destroy(t->shell);wl_compositor_destroy(t->compositor);wl_display_flush(t->display);wl_display_disconnect(t->display);free(t);}
