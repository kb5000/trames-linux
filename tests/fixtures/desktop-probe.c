// SPDX-License-Identifier: GPL-3.0-or-later
#include <xcb/xcb.h>
#include <xcb/xtest.h>
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static xcb_window_t find(xcb_connection_t*c,xcb_window_t root,const char*title){xcb_query_tree_reply_t*t=xcb_query_tree_reply(c,xcb_query_tree(c,root),NULL);assert(t);xcb_window_t id=0,*children=xcb_query_tree_children(t);for(int i=0;i<xcb_query_tree_children_length(t);i++){xcb_get_property_reply_t*r=xcb_get_property_reply(c,xcb_get_property(c,0,children[i],XCB_ATOM_WM_NAME,XCB_ATOM_STRING,0,256),NULL);if(r&&(!strcmp(title,"*")||(xcb_get_property_value_length(r)==(int)strlen(title)&&!memcmp(xcb_get_property_value(r),title,strlen(title)))))id=children[i];free(r);}free(t);return id;}
int main(int argc,char**argv){assert(argc>=3);int screen; xcb_connection_t*c=xcb_connect(NULL,&screen);assert(!xcb_connection_has_error(c));xcb_screen_iterator_t i=xcb_setup_roots_iterator(xcb_get_setup(c));while(screen--)xcb_screen_next(&i);xcb_window_t root=i.data->root,id=find(c,root,argv[1]);if(!id)return 2;const char*mode=argv[2];
if(!strcmp(mode,"pixels")){xcb_get_geometry_reply_t*g=xcb_get_geometry_reply(c,xcb_get_geometry(c,id),NULL);assert(g);xcb_get_image_reply_t*r=xcb_get_image_reply(c,xcb_get_image(c,XCB_IMAGE_FORMAT_Z_PIXMAP,id,g->width/2,g->height/2,1,1,~0u),NULL);assert(r);unsigned pixel=0;memcpy(&pixel,xcb_get_image_data(r),4);printf("pixels %u %u %06x\n",g->width,g->height,pixel&0xffffff);free(r);free(g);}
else if(!strcmp(mode,"focus")){xcb_set_input_focus(c,XCB_INPUT_FOCUS_PARENT,id,XCB_CURRENT_TIME);}
else if(!strcmp(mode,"unfocus")){xcb_set_input_focus(c,XCB_INPUT_FOCUS_PARENT,root,XCB_CURRENT_TIME);}
else if(!strcmp(mode,"key")||!strcmp(mode,"button")){assert(argc==5);int detail=atoi(argv[3]),pressed=atoi(argv[4]);xcb_test_fake_input(c,!strcmp(mode,"key")?(pressed?XCB_KEY_PRESS:XCB_KEY_RELEASE):(pressed?XCB_BUTTON_PRESS:XCB_BUTTON_RELEASE),(uint8_t)detail,XCB_CURRENT_TIME,root,0,0,0);}
else if(!strcmp(mode,"motion")){assert(argc==5);xcb_translate_coordinates_reply_t*t=xcb_translate_coordinates_reply(c,xcb_translate_coordinates(c,id,root,atoi(argv[3]),atoi(argv[4])),NULL);assert(t);xcb_test_fake_input(c,XCB_MOTION_NOTIFY,0,XCB_CURRENT_TIME,root,t->dst_x,t->dst_y,0);free(t);}
else if(!strcmp(mode,"resize")){assert(argc==5);uint32_t size[2]={(uint32_t)atoi(argv[3]),(uint32_t)atoi(argv[4])};xcb_configure_window(c,id,XCB_CONFIG_WINDOW_WIDTH|XCB_CONFIG_WINDOW_HEIGHT,size);}
else if(!strcmp(mode,"expose")){xcb_clear_area(c,1,id,0,0,0,0);}
else if(!strcmp(mode,"close")){xcb_intern_atom_reply_t*p=xcb_intern_atom_reply(c,xcb_intern_atom(c,0,12,"WM_PROTOCOLS"),NULL),*d=xcb_intern_atom_reply(c,xcb_intern_atom(c,0,16,"WM_DELETE_WINDOW"),NULL);assert(p&&d);xcb_client_message_event_t e={.response_type=XCB_CLIENT_MESSAGE,.format=32,.window=id,.type=p->atom};e.data.data32[0]=d->atom;xcb_send_event(c,0,id,0,(char*)&e);free(p);free(d);}
else return 3;
xcb_get_input_focus_reply_t*f=xcb_get_input_focus_reply(c,xcb_get_input_focus(c),NULL);free(f);xcb_disconnect(c);return 0;}
