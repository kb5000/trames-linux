// SPDX-License-Identifier: GPL-3.0-or-later
#define main desktop_main
#include "desktop-client.c"
#undef main
#include <poll.h>
// Stable public libxkbcommon ABI, keeping this independent protocol peer free
// of distribution-specific development include paths.
struct xkb_context;struct xkb_keymap;struct xkb_state;
extern struct xkb_context*xkb_context_new(int);
extern struct xkb_keymap*xkb_keymap_new_from_string(struct xkb_context*,const char*,int,int);
extern void xkb_keymap_unref(struct xkb_keymap*);
extern struct xkb_state*xkb_state_new(struct xkb_keymap*);
extern void xkb_state_unref(struct xkb_state*);
extern int xkb_state_update_mask(struct xkb_state*,uint32_t,uint32_t,uint32_t,uint32_t,uint32_t,uint32_t);
extern int xkb_state_key_get_utf8(struct xkb_state*,uint32_t,char*,size_t);
extern uint32_t xkb_state_key_get_one_sym(struct xkb_state*,uint32_t);
static struct xkb_context*context;static struct xkb_keymap*map;static struct xkb_state*state;
static void kmap(void*d,struct wl_keyboard*k,uint32_t format,int fd,uint32_t size){(void)d;(void)k;assert(format==WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1&&size>1);char*bytes=malloc(size);assert(bytes);size_t at=0;while(at<size){ssize_t n=read(fd,bytes+at,size-at);assert(n>0);at+=(size_t)n;}close(fd);assert(bytes[size-1]==0);if(state)xkb_state_unref(state);if(map)xkb_keymap_unref(map);map=xkb_keymap_new_from_string(context,bytes,1,0);assert(map);state=xkb_state_new(map);assert(state);free(bytes);printf("keymap q=%x y=%x\n",xkb_state_key_get_one_sym(state,24),xkb_state_key_get_one_sym(state,29));}
static void kmods(void*d,struct wl_keyboard*k,uint32_t serial,uint32_t dep,uint32_t lat,uint32_t lock,uint32_t group){(void)d;(void)k;(void)serial;assert(state);xkb_state_update_mask(state,dep,lat,lock,0,0,group);printf("state %u %u %u %u\n",dep,lat,lock,group);}
static void kkey(void*d,struct wl_keyboard*k,uint32_t serial,uint32_t t,uint32_t code,uint32_t pressed){(void)d;(void)k;(void)serial;(void)t;assert(state);char text[128]={0};xkb_state_key_get_utf8(state,code+8,text,sizeof(text));printf("physical %u %u sym=%x text=%s\n",code,pressed,xkb_state_key_get_one_sym(state,code+8),text);}
static void krepeat(void*d,struct wl_keyboard*k,int32_t rate,int32_t delay){(void)d;(void)k;printf("repeat %d %d\n",rate,delay);}
static const struct wl_keyboard_listener listener={kmap,enter,leave,kkey,kmods,krepeat};
static void caps(void*d,struct wl_seat*s,uint32_t c){(void)d;if(c&WL_SEAT_CAPABILITY_KEYBOARD){struct wl_keyboard*k=wl_seat_get_keyboard(s);wl_keyboard_add_listener(k,&listener,NULL);}}
static const struct wl_seat_listener slistener={caps,seat_name};
static void globals(void*d,struct wl_registry*r,uint32_t id,const char*n,uint32_t v){if(!strcmp(n,"wl_seat")){seat=wl_registry_bind(r,id,&wl_seat_interface,v<7?v:7);wl_seat_add_listener(seat,&slistener,NULL);}else global(d,r,id,n,v);}
static const struct wl_registry_listener rlistener={globals,removed};
int main(void){setbuf(stdout,NULL);context=xkb_context_new(0);assert(context);display=wl_display_connect(NULL);assert(display);struct wl_registry*r=wl_display_get_registry(display);wl_registry_add_listener(r,&rlistener,NULL);assert(wl_display_roundtrip(display)>=0);assert(compositor&&shm&&shell&&seat);surface=wl_compositor_create_surface(compositor);xdg=xdg_wm_base_get_xdg_surface(shell,surface);xdg_surface_add_listener(xdg,&surface_listener,NULL);struct xdg_toplevel*top=xdg_surface_get_toplevel(xdg);xdg_toplevel_add_listener(top,&top_listener,NULL);xdg_toplevel_set_title(top,"Trames desktop acceptance");wl_surface_commit(surface);while(!done){assert(wl_display_dispatch_pending(display)>=0);assert(wl_display_flush(display)>=0);struct pollfd fds[2]={{wl_display_get_fd(display),POLLIN,0},{0,POLLIN,0}};assert(poll(fds,2,30000)>0);if(fds[0].revents)assert(wl_display_dispatch(display)>=0);if(fds[1].revents&POLLIN){char q;assert(read(0,&q,1)==1&&q=='q');done=1;}}xdg_toplevel_destroy(top);xdg_surface_destroy(xdg);wl_surface_destroy(surface);wl_display_flush(display);wl_display_disconnect(display);puts("keyboard-done");return 0;}
