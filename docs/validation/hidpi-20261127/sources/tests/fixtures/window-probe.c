// SPDX-License-Identifier: GPL-3.0-or-later
#include <xcb/xcb.h>
#include <xcb/xtest.h>
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static xcb_atom_t atom(xcb_connection_t*c,const char*n){xcb_intern_atom_reply_t*r=xcb_intern_atom_reply(c,xcb_intern_atom(c,0,strlen(n),n),NULL);assert(r);xcb_atom_t a=r->atom;free(r);return a;}
static unsigned property(xcb_connection_t*c,xcb_window_t w,xcb_atom_t a,unsigned wanted){xcb_get_property_reply_t*p=xcb_get_property_reply(c,xcb_get_property(c,0,w,a,XCB_GET_PROPERTY_TYPE_ANY,0,64),NULL);unsigned found=0;if(p&&p->format==32){unsigned*v=xcb_get_property_value(p);int n=xcb_get_property_value_length(p)/4;if(wanted){for(int i=0;i<n;i++)found|=v[i]==wanted;}else if(n)found=v[0];}free(p);return found;}
static xcb_window_t find(xcb_connection_t*c,xcb_window_t root,const char*title){
 xcb_query_tree_reply_t*t=xcb_query_tree_reply(c,xcb_query_tree(c,root),NULL);assert(t);xcb_window_t id=0,*children=xcb_query_tree_children(t);
 for(int i=0;i<xcb_query_tree_children_length(t);i++){
  if(!strcmp(title,"*")){xcb_get_window_attributes_reply_t*a=xcb_get_window_attributes_reply(c,xcb_get_window_attributes(c,children[i]),NULL);if(a&&a->_class==XCB_WINDOW_CLASS_INPUT_OUTPUT&&a->map_state==XCB_MAP_STATE_VIEWABLE)id=children[i];free(a);continue;}
  xcb_get_property_reply_t*r=xcb_get_property_reply(c,xcb_get_property(c,0,children[i],XCB_ATOM_WM_NAME,XCB_GET_PROPERTY_TYPE_ANY,0,256),NULL);
  if(r&&xcb_get_property_value_length(r)==(int)strlen(title)&&!memcmp(xcb_get_property_value(r),title,strlen(title))){id=children[i];}
  free(r);
  if(!id&&strcmp(title,"*"))id=find(c,children[i],title);
  if(id)break;
 }free(t);return id;
}
int main(int argc,char**argv){assert(argc>=3);int screen; xcb_connection_t*c=xcb_connect(NULL,&screen);assert(!xcb_connection_has_error(c));xcb_screen_iterator_t i=xcb_setup_roots_iterator(xcb_get_setup(c));while(screen--)xcb_screen_next(&i);xcb_window_t root=i.data->root;if(!strcmp(argv[2],"wm")){unsigned id=property(c,root,atom(c,"_NET_SUPPORTING_WM_CHECK"),0);xcb_disconnect(c);return id?0:2;}xcb_window_t id=find(c,root,argv[1]);if(!id)return 2;const char*mode=argv[2];if(!strcmp(mode,"geometry")||(!strcmp(mode,"rect")||!strcmp(mode,"rect-red"))){xcb_get_window_attributes_reply_t*a=xcb_get_window_attributes_reply(c,xcb_get_window_attributes(c,id),NULL);if(!a||a->map_state!=XCB_MAP_STATE_VIEWABLE){free(a);xcb_disconnect(c);return 2;}free(a);}
if(!strcmp(mode,"id")){printf("%u\n",id);}
else if(!strcmp(mode,"parent")){printf("%u\n",property(c,id,XCB_ATOM_WM_TRANSIENT_FOR,0));}
else if(!strcmp(mode,"state")){xcb_atom_t a=atom(c,"_NET_WM_STATE");printf("%u %u %u\n",property(c,id,a,atom(c,"_NET_WM_STATE_MAXIMIZED_HORZ"))&&property(c,id,a,atom(c,"_NET_WM_STATE_MAXIMIZED_VERT")),property(c,id,a,atom(c,"_NET_WM_STATE_FULLSCREEN")),property(c,id,atom(c,"WM_STATE"),0)==3);}
else if(!strcmp(mode,"restore")){xcb_map_window(c,id);xcb_client_message_event_t e={.response_type=XCB_CLIENT_MESSAGE,.format=32,.window=id,.type=atom(c,"_NET_ACTIVE_WINDOW")};e.data.data32[0]=2;xcb_send_event(c,0,root,XCB_EVENT_MASK_SUBSTRUCTURE_REDIRECT|XCB_EVENT_MASK_SUBSTRUCTURE_NOTIFY,(char*)&e);}
else if(!strcmp(mode,"point")){assert(argc==5);xcb_test_fake_input(c,XCB_MOTION_NOTIFY,0,XCB_CURRENT_TIME,root,atoi(argv[3]),atoi(argv[4]),0);}
else if(!strcmp(mode,"geometry")||(!strcmp(mode,"rect")||!strcmp(mode,"rect-red"))){xcb_get_geometry_reply_t*g=xcb_get_geometry_reply(c,xcb_get_geometry(c,id),NULL);assert(g);xcb_translate_coordinates_reply_t*t=xcb_translate_coordinates_reply(c,xcb_translate_coordinates(c,id,root,0,0),NULL);assert(t);int x=0,y=0,w=g->width,h=g->height;if((!strcmp(mode,"rect")||!strcmp(mode,"rect-red"))){xcb_get_image_reply_t*p=xcb_get_image_reply(c,xcb_get_image(c,XCB_IMAGE_FORMAT_Z_PIXMAP,id,0,0,g->width,g->height,~0u),NULL);assert(p);unsigned*v=(unsigned*)xcb_get_image_data(p);int l=w,r=-1,top=h,b=-1;for(int yy=0;yy<h;yy++)for(int xx=0;xx<w;xx++)if((v[yy*w+xx]&0xffffff)==(!strcmp(mode,"rect-red")?0xc04020u:0x20c040u)){if(xx<l)l=xx;if(xx>r)r=xx;if(yy<top)top=yy;if(yy>b)b=yy;}free(p);if(r<l){free(g);free(t);xcb_disconnect(c);return 2;}x=l;y=top;w=r-l+1;h=b-top+1;}printf("%d %d %d %d\n",t->dst_x+x,t->dst_y+y,w,h);free(t);free(g);}
else if(!strcmp(mode,"pixels")){xcb_get_geometry_reply_t*g=xcb_get_geometry_reply(c,xcb_get_geometry(c,id),NULL);assert(g);xcb_get_image_reply_t*r=xcb_get_image_reply(c,xcb_get_image(c,XCB_IMAGE_FORMAT_Z_PIXMAP,id,g->width/2,g->height/2,1,1,~0u),NULL);assert(r);unsigned pixel=0;memcpy(&pixel,xcb_get_image_data(r),4);printf("pixels %u %u %06x\n",g->width,g->height,pixel&0xffffff);free(r);free(g);}
else if(!strcmp(mode,"focus")){xcb_set_input_focus(c,XCB_INPUT_FOCUS_PARENT,id,XCB_CURRENT_TIME);}
else if(!strcmp(mode,"unfocus")){xcb_set_input_focus(c,XCB_INPUT_FOCUS_PARENT,root,XCB_CURRENT_TIME);}
else if(!strcmp(mode,"key")||!strcmp(mode,"button")){assert(argc==5);int detail=atoi(argv[3]),pressed=atoi(argv[4]);xcb_test_fake_input(c,!strcmp(mode,"key")?(pressed?XCB_KEY_PRESS:XCB_KEY_RELEASE):(pressed?XCB_BUTTON_PRESS:XCB_BUTTON_RELEASE),(uint8_t)detail,XCB_CURRENT_TIME,root,0,0,0);}
else if(!strcmp(mode,"motion")){assert(argc==5);xcb_translate_coordinates_reply_t*t=xcb_translate_coordinates_reply(c,xcb_translate_coordinates(c,id,root,atoi(argv[3]),atoi(argv[4])),NULL);assert(t);xcb_test_fake_input(c,XCB_MOTION_NOTIFY,0,XCB_CURRENT_TIME,root,t->dst_x,t->dst_y,0);free(t);}
else if(!strcmp(mode,"resize")){assert(argc==5);uint32_t size[2]={(uint32_t)atoi(argv[3]),(uint32_t)atoi(argv[4])};xcb_configure_window(c,id,XCB_CONFIG_WINDOW_WIDTH|XCB_CONFIG_WINDOW_HEIGHT,size);}
else if(!strcmp(mode,"expose")){xcb_clear_area(c,1,id,0,0,0,0);}
else if(!strcmp(mode,"close")){xcb_intern_atom_reply_t*p=xcb_intern_atom_reply(c,xcb_intern_atom(c,0,12,"WM_PROTOCOLS"),NULL),*d=xcb_intern_atom_reply(c,xcb_intern_atom(c,0,16,"WM_DELETE_WINDOW"),NULL);assert(p&&d);xcb_client_message_event_t e={.response_type=XCB_CLIENT_MESSAGE,.format=32,.window=id,.type=p->atom};e.data.data32[0]=d->atom;xcb_send_event(c,0,id,0,(char*)&e);free(p);free(d);}
else return 3;
xcb_get_input_focus_reply_t*f=xcb_get_input_focus_reply(c,xcb_get_input_focus(c),NULL);free(f);xcb_disconnect(c);return 0;}
