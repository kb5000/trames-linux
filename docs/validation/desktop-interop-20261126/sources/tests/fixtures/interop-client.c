// SPDX-License-Identifier: GPL-3.0-or-later
// Original libwayland protocol peer for clipboard/text. All interaction crosses
// the real socket; no compositor state is injected by this fixture.
#define main base_main
#include "desktop-client.c"
#undef main
#include "text-input-v3-client-protocol.h"
#include <poll.h>
static struct wl_data_device_manager *data_manager;
static struct wl_data_device *data_device;
static struct wl_data_offer *selection;
static struct wl_data_source *source;
static struct zwp_text_input_manager_v3 *text_manager;
static struct zwp_text_input_v3 *text;
static uint32_t input_serial;
static int incoming=-1;
static char incoming_text[20000];static size_t incoming_length;
static char clipboard[16385]="guest 剪贴板 ✅";
static void source_target(void*d,struct wl_data_source*s,const char*m){(void)d;(void)s;(void)m;}
static void source_send(void*d,struct wl_data_source*s,const char*m,int fd){(void)d;(void)s;(void)m;size_t n=strlen(clipboard),at=0;while(at<n){ssize_t r=write(fd,clipboard+at,n-at);if(r<=0)break;at+=(size_t)r;}close(fd);puts("clipboard-sent");}
static void source_cancel(void*d,struct wl_data_source*s){(void)d;(void)s;puts("clipboard-cancelled");}
static const struct wl_data_source_listener source_listener={.target=source_target,.send=source_send,.cancelled=source_cancel};
static void offer_mime(void*d,struct wl_data_offer*o,const char*m){(void)d;(void)o;(void)m;}
static const struct wl_data_offer_listener offer_listener={.offer=offer_mime};
static void data_offer(void*d,struct wl_data_device*p,struct wl_data_offer*o){(void)d;(void)p;wl_data_offer_add_listener(o,&offer_listener,NULL);}
static void data_enter(void*d,struct wl_data_device*p,uint32_t s,struct wl_surface*w,wl_fixed_t x,wl_fixed_t y,struct wl_data_offer*o){(void)d;(void)p;(void)s;(void)w;(void)x;(void)y;(void)o;}
static void data_leave(void*d,struct wl_data_device*p){(void)d;(void)p;}
static void data_motion(void*d,struct wl_data_device*p,uint32_t t,wl_fixed_t x,wl_fixed_t y){(void)d;(void)p;(void)t;(void)x;(void)y;}
static void data_drop(void*d,struct wl_data_device*p){(void)d;(void)p;}
static void data_selection(void*d,struct wl_data_device*p,struct wl_data_offer*o){(void)d;(void)p;if(selection)wl_data_offer_destroy(selection);selection=o;printf("selection %d\n",o!=NULL);}
static const struct wl_data_device_listener data_listener={data_offer,data_enter,data_leave,data_motion,data_drop,data_selection};
static void text_enter(void*d,struct zwp_text_input_v3*t,struct wl_surface*w){(void)d;(void)t;(void)w;puts("text-enter");}
static void text_leave(void*d,struct zwp_text_input_v3*t,struct wl_surface*w){(void)d;(void)t;(void)w;puts("text-leave");}
static void text_preedit(void*d,struct zwp_text_input_v3*t,const char*s,int32_t a,int32_t b){(void)d;(void)t;printf("preedit %s %d %d\n",s?s:"",a,b);}
static void text_commit(void*d,struct zwp_text_input_v3*t,const char*s){(void)d;(void)t;printf("commit %s\n",s?s:"");}
static void text_delete(void*d,struct zwp_text_input_v3*t,uint32_t a,uint32_t b){(void)d;(void)t;printf("delete %u %u\n",a,b);}
static void text_done(void*d,struct zwp_text_input_v3*t,uint32_t s){(void)d;(void)t;printf("text-done %u\n",s);}
static const struct zwp_text_input_v3_listener text_listener={text_enter,text_leave,text_preedit,text_commit,text_delete,text_done};
static void interop_enter(void*d,struct wl_keyboard*k,uint32_t s,struct wl_surface*w,struct wl_array*a){enter(d,k,s,w,a);input_serial=s;}
static void interop_key(void*d,struct wl_keyboard*k,uint32_t s,uint32_t t,uint32_t code,uint32_t state){input_serial=s;key(d,k,s,t,code,state);}
static const struct wl_keyboard_listener interop_keyboard={keymap,interop_enter,leave,interop_key,modifiers,repeat};
static void interop_caps(void*d,struct wl_seat*s,uint32_t caps){(void)d;if(caps&WL_SEAT_CAPABILITY_KEYBOARD){struct wl_keyboard*k=wl_seat_get_keyboard(s);wl_keyboard_add_listener(k,&interop_keyboard,NULL);}if(caps&WL_SEAT_CAPABILITY_POINTER){struct wl_pointer*p=wl_seat_get_pointer(s);wl_pointer_add_listener(p,&pointer_listener,NULL);}}
static const struct wl_seat_listener interop_seat={interop_caps,seat_name};
static void interop_global(void*d,struct wl_registry*r,uint32_t id,const char*n,uint32_t v){
 if(!strcmp(n,"wl_seat")){seat=wl_registry_bind(r,id,&wl_seat_interface,v<7?v:7);wl_seat_add_listener(seat,&interop_seat,NULL);return;}global(d,r,id,n,v);
 if(!strcmp(n,"wl_data_device_manager"))data_manager=wl_registry_bind(r,id,&wl_data_device_manager_interface,v<3?v:3);
 if(!strcmp(n,"zwp_text_input_manager_v3"))text_manager=wl_registry_bind(r,id,&zwp_text_input_manager_v3_interface,1);
}
static const struct wl_registry_listener interop_registry={interop_global,removed};
static void command(char*s){
 if(!strcmp(s,"copy")){assert(input_serial);if(source)wl_data_source_destroy(source);source=wl_data_device_manager_create_data_source(data_manager);wl_data_source_add_listener(source,&source_listener,NULL);wl_data_source_offer(source,"text/plain;charset=utf-8");wl_data_source_offer(source,"text/plain");wl_data_device_set_selection(data_device,source,input_serial);puts("copy-ready");}
 else if(!strncmp(s,"value ",6)){assert(strlen(s+6)<sizeof(clipboard));strcpy(clipboard,s+6);}
 else if(!strcmp(s,"paste")){assert(selection&&incoming<0);int fds[2];assert(!pipe2(fds,O_CLOEXEC));incoming=fds[0];assert(fcntl(incoming,F_SETFL,O_NONBLOCK)>=0);incoming_length=0;wl_data_offer_receive(selection,"text/plain;charset=utf-8",fds[1]);close(fds[1]);}
 else if(!strcmp(s,"text")){assert(text);zwp_text_input_v3_enable(text);zwp_text_input_v3_set_surrounding_text(text,"a中文b",7,7);zwp_text_input_v3_set_cursor_rectangle(text,25,35,2,20);zwp_text_input_v3_commit(text);puts("text-ready");}
 else if(!strcmp(s,"disable")){assert(text);zwp_text_input_v3_disable(text);zwp_text_input_v3_commit(text);}
 else if(!strcmp(s,"hide")){hidden=1;wl_surface_attach(surface,NULL,0,0);wl_surface_commit(surface);assert(wl_display_roundtrip(display)>=0);puts("hidden");}
 else if(!strcmp(s,"show")){hidden=0;draw();}
 else if(!strcmp(s,"quit")){done=1;}
 else abort();
}
int main(void){setbuf(stdout,NULL);display=wl_display_connect(NULL);assert(display);struct wl_registry*r=wl_display_get_registry(display);wl_registry_add_listener(r,&interop_registry,NULL);assert(wl_display_roundtrip(display)>=0);assert(compositor&&shm&&shell&&seat&&data_manager);data_device=wl_data_device_manager_get_data_device(data_manager,seat);wl_data_device_add_listener(data_device,&data_listener,NULL);if(text_manager){text=zwp_text_input_manager_v3_get_text_input(text_manager,seat);zwp_text_input_v3_add_listener(text,&text_listener,NULL);}surface=wl_compositor_create_surface(compositor);xdg=xdg_wm_base_get_xdg_surface(shell,surface);xdg_surface_add_listener(xdg,&surface_listener,NULL);struct xdg_toplevel*top=xdg_surface_get_toplevel(xdg);xdg_toplevel_add_listener(top,&top_listener,NULL);xdg_toplevel_set_title(top,"Trames desktop acceptance");wl_surface_commit(surface);
 int flags=fcntl(0,F_GETFL);assert(fcntl(0,F_SETFL,flags|O_NONBLOCK)>=0);char commands[20000];size_t used=0;
 while(!done){assert(wl_display_dispatch_pending(display)>=0);assert(wl_display_flush(display)>=0);struct pollfd fds[3]={{wl_display_get_fd(display),POLLIN,0},{0,POLLIN,0},{incoming,POLLIN,0}};assert(poll(fds,3,30000)>0);if(fds[0].revents)assert(wl_display_dispatch(display)>=0);
 if(fds[1].revents&POLLIN){ssize_t n=read(0,commands+used,sizeof(commands)-used-1);assert(n>0);used+=(size_t)n;commands[used]=0;char*start=commands,*end;while((end=strchr(start,'\n'))){*end=0;command(start);start=end+1;}used-=start-commands;memmove(commands,start,used);}
 if(incoming>=0&&fds[2].revents){ssize_t n=read(incoming,incoming_text+incoming_length,sizeof(incoming_text)-incoming_length-1);if(n==0){incoming_text[incoming_length]=0;printf("paste %s\n",incoming_text);close(incoming);incoming=-1;}else if(n>0){incoming_length+=(size_t)n;assert(incoming_length<sizeof(incoming_text)-1);}}
 }
 if(text){zwp_text_input_v3_destroy(text);}
 if(source){wl_data_source_destroy(source);}
 xdg_toplevel_destroy(top);xdg_surface_destroy(xdg);wl_surface_destroy(surface);wl_display_flush(display);wl_display_disconnect(display);puts("interop-done");return 0;}
