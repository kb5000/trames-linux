// SPDX-License-Identifier: GPL-3.0-or-later
// Original XTEST key sequence. Key-up remains valid after the receiving window
// exits on the committed text; it does not require finding that window again.
#include <xcb/xcb.h>
#include <xcb/xtest.h>
#include <assert.h>
#include <stdlib.h>
int main(void){int screen; xcb_connection_t*c=xcb_connect(NULL,&screen);assert(!xcb_connection_has_error(c));xcb_screen_iterator_t i=xcb_setup_roots_iterator(xcb_get_setup(c));while(screen--)xcb_screen_next(&i);const unsigned char codes[]={39,39,36,36};xcb_void_cookie_t sent[4];for(int n=0;n<4;n++)sent[n]=xcb_test_fake_input_checked(c,n%2?XCB_KEY_RELEASE:XCB_KEY_PRESS,codes[n],XCB_CURRENT_TIME,i.data->root,0,0,0);for(int n=0;n<4;n++){xcb_generic_error_t*e=xcb_request_check(c,sent[n]);assert(!e);free(e);}xcb_disconnect(c);return 0;}
