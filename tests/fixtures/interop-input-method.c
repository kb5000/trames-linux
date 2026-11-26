// SPDX-License-Identifier: GPL-3.0-or-later
// A real input-method-v1 provider launched by original Weston.
#include <wayland-client.h>
#include "input-method-v1-client-protocol.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
static struct wl_display*display;static struct zwp_input_method_v1*method;static int sent;
static void surrounding(void*d,struct zwp_input_method_context_v1*c,const char*t,uint32_t a,uint32_t b){(void)d;(void)c;printf("surrounding %s %u %u\n",t,a,b);}
static void reset(void*d,struct zwp_input_method_context_v1*c){(void)d;(void)c;}
static void content(void*d,struct zwp_input_method_context_v1*c,uint32_t h,uint32_t p){(void)d;(void)c;(void)h;(void)p;}
static void action(void*d,struct zwp_input_method_context_v1*c,uint32_t b,uint32_t i){(void)d;(void)c;(void)b;(void)i;}
static void state(void*d,struct zwp_input_method_context_v1*c,uint32_t serial){(void)d;if(!sent){zwp_input_method_context_v1_preedit_cursor(c,6);zwp_input_method_context_v1_preedit_string(c,serial,"拼音","");zwp_input_method_context_v1_delete_surrounding_text(c,-3,3);zwp_input_method_context_v1_commit_string(c,serial,"中文输入 ✅");sent=1;puts("method-sent");}}
static void language(void*d,struct zwp_input_method_context_v1*c,const char*l){(void)d;(void)c;(void)l;}
static const struct zwp_input_method_context_v1_listener context_listener={surrounding,reset,content,action,state,language};
static void activate(void*d,struct zwp_input_method_v1*m,struct zwp_input_method_context_v1*c){(void)d;(void)m;sent=0;zwp_input_method_context_v1_add_listener(c,&context_listener,NULL);puts("method-active");}
static void deactivate(void*d,struct zwp_input_method_v1*m,struct zwp_input_method_context_v1*c){(void)d;(void)m;zwp_input_method_context_v1_destroy(c);}
static const struct zwp_input_method_v1_listener method_listener={activate,deactivate};
static void global(void*d,struct wl_registry*r,uint32_t n,const char*i,uint32_t v){(void)d;(void)v;if(!strcmp(i,"zwp_input_method_v1")){method=wl_registry_bind(r,n,&zwp_input_method_v1_interface,1);zwp_input_method_v1_add_listener(method,&method_listener,NULL);}}
static void removed(void*d,struct wl_registry*r,uint32_t n){(void)d;(void)r;(void)n;}
static const struct wl_registry_listener registry_listener={global,removed};
int main(void){setbuf(stdout,NULL);display=wl_display_connect(NULL);assert(display);struct wl_registry*r=wl_display_get_registry(display);wl_registry_add_listener(r,&registry_listener,NULL);assert(wl_display_roundtrip(display)>=0);assert(method);puts("method-ready");while(wl_display_dispatch(display)>=0){}return 0;}
