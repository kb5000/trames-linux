// SPDX-License-Identifier: GPL-3.0-or-later
#include <xcb/xcb.h>
#include <assert.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
static xcb_atom_t atom(xcb_connection_t*c,const char*s){xcb_intern_atom_reply_t*r=xcb_intern_atom_reply(c,xcb_intern_atom(c,0,strlen(s),s),NULL);assert(r);xcb_atom_t a=r->atom;free(r);return a;}
int main(int argc,char**argv){assert(argc>=2);setbuf(stdout,NULL);int screen;xcb_connection_t*c=xcb_connect(NULL,&screen);assert(!xcb_connection_has_error(c));xcb_screen_iterator_t it=xcb_setup_roots_iterator(xcb_get_setup(c));while(screen--)xcb_screen_next(&it);xcb_window_t w=xcb_generate_id(c);uint32_t mask=XCB_EVENT_MASK_PROPERTY_CHANGE;xcb_create_window(c,0,w,it.data->root,0,0,1,1,0,XCB_WINDOW_CLASS_INPUT_ONLY,0,XCB_CW_EVENT_MASK,&mask);xcb_atom_t selection=atom(c,"CLIPBOARD"),utf8=atom(c,"UTF8_STRING"),targets=atom(c,"TARGETS"),property=atom(c,"_INTEROP_CLIP");int own=!strcmp(argv[1],"own"),latin=argc>3&&!strcmp(argv[3],"latin1");if(own){assert(argc>=3);xcb_set_selection_owner(c,w,selection,XCB_CURRENT_TIME);puts("owner-ready");}else xcb_convert_selection(c,w,selection,utf8,property,XCB_CURRENT_TIME);xcb_flush(c);
 while(1){xcb_generic_event_t*e;while((e=xcb_poll_for_event(c))){unsigned type=e->response_type&127;
 if(type==XCB_SELECTION_REQUEST){xcb_selection_request_event_t*r=(void*)e;xcb_atom_t p=r->property?r->property:r->target,accepted=XCB_NONE;if(r->target==targets){xcb_atom_t a[]={targets,latin?XCB_ATOM_STRING:utf8};xcb_change_property(c,XCB_PROP_MODE_REPLACE,r->requestor,p,XCB_ATOM_ATOM,32,2,a);accepted=p;}else if(r->target==(latin?XCB_ATOM_STRING:utf8)){xcb_change_property(c,XCB_PROP_MODE_REPLACE,r->requestor,p,r->target,8,strlen(argv[2]),argv[2]);accepted=p;puts("clipboard-sent");}xcb_selection_notify_event_t n={.response_type=XCB_SELECTION_NOTIFY,.time=r->time,.requestor=r->requestor,.selection=r->selection,.target=r->target,.property=accepted};xcb_send_event(c,0,r->requestor,0,(char*)&n);xcb_flush(c);}
 if(type==XCB_SELECTION_NOTIFY&&!own){xcb_selection_notify_event_t*n=(void*)e;assert(n->property);xcb_get_property_reply_t*r=xcb_get_property_reply(c,xcb_get_property(c,1,w,property,XCB_GET_PROPERTY_TYPE_ANY,0,5000),NULL);assert(r&&r->format==8&&!r->bytes_after);fwrite(xcb_get_property_value(r),1,xcb_get_property_value_length(r),stdout);puts("");free(r);free(e);xcb_disconnect(c);return 0;}free(e);}
 struct pollfd fds[2]={{xcb_get_file_descriptor(c),POLLIN,0},{own?0:-1,POLLIN,0}};assert(poll(fds,2,30000)>0);if(fds[1].revents){char b;if(read(0,&b,1)<=0||b=='q')break;}}
 xcb_disconnect(c);return 0;}
