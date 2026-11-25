// SPDX-License-Identifier: GPL-3.0-or-later
// Read actual server pixels, or send native resize/close events, for acceptance.
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <xcb/xcb.h>
static xcb_window_t find(xcb_connection_t* c,xcb_window_t root,const char* name) {
    xcb_query_tree_reply_t* tree=xcb_query_tree_reply(c,xcb_query_tree(c,root),NULL);assert(tree);
    int n=xcb_query_tree_children_length(tree);xcb_window_t* children=xcb_query_tree_children(tree);xcb_window_t result=0;
    for(int i=0;i<n;i++) {
        xcb_get_property_reply_t* p=xcb_get_property_reply(c,xcb_get_property(c,0,children[i],XCB_ATOM_WM_NAME,XCB_ATOM_STRING,0,256),NULL);
        if(p && (size_t)xcb_get_property_value_length(p)==strlen(name) && !memcmp(xcb_get_property_value(p),name,strlen(name))){result=children[i];}
        free(p);
        if(result)break;
    }
    free(tree);return result;
}
int main(int argc,char** argv) {
    assert(argc>=2);int screen_number;xcb_connection_t* c=xcb_connect(NULL,&screen_number);assert(!xcb_connection_has_error(c));xcb_screen_iterator_t screens=xcb_setup_roots_iterator(xcb_get_setup(c));for(int i=0;i<screen_number;i++)xcb_screen_next(&screens);
    xcb_window_t window=find(c,screens.data->root,argv[1]);assert(window);
    if(argc==4) {
        uint32_t sizes[]={(uint32_t)atoi(argv[2]),(uint32_t)atoi(argv[3])};xcb_configure_window(c,window,XCB_CONFIG_WINDOW_WIDTH|XCB_CONFIG_WINDOW_HEIGHT,sizes);xcb_flush(c);
    } else if(argc==3 && !strcmp(argv[2],"close")) {
        xcb_intern_atom_reply_t* protocol=xcb_intern_atom_reply(c,xcb_intern_atom(c,0,12,"WM_PROTOCOLS"),NULL);xcb_intern_atom_reply_t* close=xcb_intern_atom_reply(c,xcb_intern_atom(c,0,16,"WM_DELETE_WINDOW"),NULL);assert(protocol&&close);
        xcb_client_message_event_t event={.response_type=XCB_CLIENT_MESSAGE,.format=32,.window=window,.type=protocol->atom,.data={.data32={close->atom,0,0,0,0}}};xcb_send_event(c,0,window,XCB_EVENT_MASK_NO_EVENT,(const char*)&event);xcb_flush(c);free(protocol);free(close);
    } else {
        xcb_get_geometry_reply_t* geometry=xcb_get_geometry_reply(c,xcb_get_geometry(c,window),NULL);assert(geometry);
        xcb_get_image_reply_t* image=xcb_get_image_reply(c,xcb_get_image(c,XCB_IMAGE_FORMAT_Z_PIXMAP,window,0,0,geometry->width,geometry->height,0xffffff),NULL);assert(image);unsigned char* data=xcb_get_image_data(image);assert(xcb_get_image_data_length(image)==geometry->width*geometry->height*4);
        for(unsigned i=0;i<(unsigned)geometry->width*geometry->height;i++) {
            if(data[i*4]!=0 || data[i*4+1]!=255 || data[i*4+2]!=0) {
                fprintf(stderr,"pixel %u: %u,%u,%u\n",i,data[i*4],data[i*4+1],data[i*4+2]);
                free(image);free(geometry);xcb_disconnect(c);return 1;
            }
        }
        printf("pixels: %ux%u green\n",geometry->width,geometry->height);free(image);free(geometry);
    }
    xcb_disconnect(c);return 0;
}
