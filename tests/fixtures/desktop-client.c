// SPDX-License-Identifier: GPL-3.0-or-later
// Original client of libwayland: the acceptance driver never calls compositor APIs.
#define _GNU_SOURCE
#include <wayland-client.h>
#include "xdg-shell-client-protocol.h"
#include <assert.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
static struct wl_display *display;
static struct wl_compositor *compositor;
static struct wl_shm *shm;
static struct xdg_wm_base *shell;
static struct wl_surface *surface;
static struct xdg_surface *xdg;
static struct wl_seat *seat;
static int width=240,height=160,done,configured,frames,hidden;
static uint32_t color=0xff20c040;
static void draw(void);
static void frame(void *d,struct wl_callback *cb,uint32_t t){(void)d;(void)t;wl_callback_destroy(cb);printf("frame %d\n",++frames);fflush(stdout);}
static const struct wl_callback_listener frame_listener={frame};
static void draw(void){
    if(!configured||hidden)return;
    int fd=memfd_create("desktop-acceptance",MFD_CLOEXEC|MFD_ALLOW_SEALING);assert(fd>=0);
    size_t size=(size_t)width*height*4;assert(!ftruncate(fd,(off_t)size));
    uint32_t *pixels=mmap(NULL,size,PROT_READ|PROT_WRITE,MAP_SHARED,fd,0);assert(pixels!=MAP_FAILED);
    for(size_t i=0;i<size/4;i++)pixels[i]=color;
    struct wl_shm_pool *pool=wl_shm_create_pool(shm,fd,(int)size);
    struct wl_buffer *buffer=wl_shm_pool_create_buffer(pool,0,width,height,width*4,WL_SHM_FORMAT_XRGB8888);
    wl_shm_pool_destroy(pool);wl_surface_attach(surface,buffer,0,0);wl_surface_damage(surface,0,0,width,height);
    struct wl_callback *cb=wl_surface_frame(surface);wl_callback_add_listener(cb,&frame_listener,NULL);wl_surface_commit(surface);
    wl_buffer_destroy(buffer);munmap(pixels,size);close(fd);
}
static void configure(void*d,struct xdg_surface*s,uint32_t serial){(void)d;xdg_surface_ack_configure(s,serial);configured=1;draw();printf("size %d %d\n",width,height);fflush(stdout);}
static const struct xdg_surface_listener surface_listener={configure};
static void top_configure(void*d,struct xdg_toplevel*t,int32_t w,int32_t h,struct wl_array*a){(void)d;(void)t;(void)a;if(w>0)width=w;if(h>0)height=h;}
static void close_top(void*d,struct xdg_toplevel*t){(void)d;(void)t;puts("desktop-done");fflush(stdout);done=1;}
static const struct xdg_toplevel_listener top_listener={.configure=top_configure,.close=close_top};
static void ping(void*d,struct xdg_wm_base*b,uint32_t s){(void)d;xdg_wm_base_pong(b,s);}
static const struct xdg_wm_base_listener shell_listener={ping};
static void keymap(void*d,struct wl_keyboard*k,uint32_t f,int fd,uint32_t s){(void)d;(void)k;(void)f;(void)s;close(fd);}
static void enter(void*d,struct wl_keyboard*k,uint32_t s,struct wl_surface*w,struct wl_array*keys){(void)d;(void)k;(void)s;(void)w;printf("focus-in keys=%zu\n",keys->size);fflush(stdout);}
static void leave(void*d,struct wl_keyboard*k,uint32_t s,struct wl_surface*w){(void)d;(void)k;(void)s;(void)w;puts("focus-out");fflush(stdout);}
static void key(void*d,struct wl_keyboard*k,uint32_t s,uint32_t t,uint32_t code,uint32_t state){(void)d;(void)k;(void)s;(void)t;printf("key %u %u\n",code,state);if(code==30&&state){color=0xffc04020;draw();}fflush(stdout);}
static void modifiers(void*d,struct wl_keyboard*k,uint32_t s,uint32_t depressed,uint32_t latched,uint32_t locked,uint32_t group){(void)d;(void)k;(void)s;(void)latched;(void)group;printf("modifiers %u %u\n",depressed,locked);fflush(stdout);}
static void repeat(void*d,struct wl_keyboard*k,int32_t rate,int32_t delay){(void)d;(void)k;(void)rate;(void)delay;}
static const struct wl_keyboard_listener keyboard_listener={keymap,enter,leave,key,modifiers,repeat};
static void pointer_enter(void*d,struct wl_pointer*p,uint32_t s,struct wl_surface*w,wl_fixed_t x,wl_fixed_t y){(void)d;(void)p;(void)s;(void)w;printf("pointer %.0f %.0f\n",wl_fixed_to_double(x),wl_fixed_to_double(y));fflush(stdout);}
static void pointer_leave(void*d,struct wl_pointer*p,uint32_t s,struct wl_surface*w){(void)d;(void)p;(void)s;(void)w;}
static void motion(void*d,struct wl_pointer*p,uint32_t t,wl_fixed_t x,wl_fixed_t y){(void)d;(void)p;(void)t;printf("pointer %.0f %.0f\n",wl_fixed_to_double(x),wl_fixed_to_double(y));fflush(stdout);}
static void button(void*d,struct wl_pointer*p,uint32_t s,uint32_t t,uint32_t b,uint32_t state){(void)d;(void)p;(void)s;(void)t;printf("button %u %u\n",b,state);if(state){color=0xff2040c0;draw();}fflush(stdout);}
static void axis(void*d,struct wl_pointer*p,uint32_t t,uint32_t a,wl_fixed_t value){(void)d;(void)p;(void)t;printf("scroll %u %.0f\n",a,wl_fixed_to_double(value));fflush(stdout);}
static void pointer_frame(void*d,struct wl_pointer*p){(void)d;(void)p;}
static void axis_source(void*d,struct wl_pointer*p,uint32_t s){(void)d;(void)p;(void)s;}
static void axis_stop(void*d,struct wl_pointer*p,uint32_t t,uint32_t a){(void)d;(void)p;(void)t;(void)a;}
static void axis_discrete(void*d,struct wl_pointer*p,uint32_t a,int32_t v){(void)d;(void)p;(void)a;(void)v;}
static const struct wl_pointer_listener pointer_listener={.enter=pointer_enter,.leave=pointer_leave,.motion=motion,.button=button,.axis=axis,.frame=pointer_frame,.axis_source=axis_source,.axis_stop=axis_stop,.axis_discrete=axis_discrete};
static void capabilities(void*d,struct wl_seat*s,uint32_t caps){(void)d;if(caps&WL_SEAT_CAPABILITY_KEYBOARD){struct wl_keyboard*k=wl_seat_get_keyboard(s);wl_keyboard_add_listener(k,&keyboard_listener,NULL);}if(caps&WL_SEAT_CAPABILITY_POINTER){struct wl_pointer*p=wl_seat_get_pointer(s);wl_pointer_add_listener(p,&pointer_listener,NULL);}}
static void seat_name(void*d,struct wl_seat*s,const char*n){(void)d;(void)s;(void)n;}
static const struct wl_seat_listener seat_listener={capabilities,seat_name};
static void global(void*d,struct wl_registry*r,uint32_t id,const char*name,uint32_t v){(void)d;if(!strcmp(name,"wl_compositor"))compositor=wl_registry_bind(r,id,&wl_compositor_interface,v<4?v:4);if(!strcmp(name,"wl_shm"))shm=wl_registry_bind(r,id,&wl_shm_interface,1);if(!strcmp(name,"xdg_wm_base")){shell=wl_registry_bind(r,id,&xdg_wm_base_interface,3);xdg_wm_base_add_listener(shell,&shell_listener,NULL);}if(!strcmp(name,"wl_seat")){seat=wl_registry_bind(r,id,&wl_seat_interface,v<7?v:7);wl_seat_add_listener(seat,&seat_listener,NULL);}}
static void removed(void*d,struct wl_registry*r,uint32_t id){(void)d;(void)r;(void)id;}
static const struct wl_registry_listener registry_listener={global,removed};
int main(void){setvbuf(stdout,NULL,_IOLBF,0);display=wl_display_connect(NULL);assert(display);struct wl_registry*r=wl_display_get_registry(display);wl_registry_add_listener(r,&registry_listener,NULL);assert(wl_display_roundtrip(display)>=0);assert(compositor&&shm&&shell&&seat);surface=wl_compositor_create_surface(compositor);xdg=xdg_wm_base_get_xdg_surface(shell,surface);xdg_surface_add_listener(xdg,&surface_listener,NULL);struct xdg_toplevel*top=xdg_surface_get_toplevel(xdg);xdg_toplevel_add_listener(top,&top_listener,NULL);xdg_toplevel_set_title(top,"Trames desktop acceptance");wl_surface_commit(surface);while(!done)assert(wl_display_dispatch(display)>=0);xdg_toplevel_destroy(top);xdg_surface_destroy(xdg);wl_surface_destroy(surface);wl_display_flush(display);wl_display_disconnect(display);return 0;}
