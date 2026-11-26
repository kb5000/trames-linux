// SPDX-License-Identifier: GPL-3.0-or-later
//! Real host protocol peer used only by the external acceptance controller.
use x11rb::{connection::Connection, protocol::xproto::KeyPressEvent};
use xim::{
    x11rb::X11rbServer, InputStyle, Server, ServerError, ServerHandler, UserInputContext,
    XimConnections,
};
type Im<'a> = X11rbServer<&'a x11rb::rust_connection::RustConnection>;
struct Handler;
impl ServerHandler<Im<'_>> for Handler {
    type InputContextData = ();
    type InputStyleArray = [InputStyle; 2];
    fn new_ic_data(&mut self, _: &mut Im<'_>, _: InputStyle) -> Result<(), ServerError> {
        Ok(())
    }
    fn input_styles(&self) -> Self::InputStyleArray {
        [
            InputStyle::PREEDIT_CALLBACKS | InputStyle::STATUS_NOTHING,
            InputStyle::PREEDIT_POSITION | InputStyle::STATUS_NOTHING,
        ]
    }
    fn filter_events(&self) -> u32 {
        1
    }
    fn handle_connect(&mut self, _: &mut Im<'_>) -> Result<(), ServerError> {
        Ok(())
    }
    fn handle_create_ic(
        &mut self,
        s: &mut Im<'_>,
        u: &mut UserInputContext<()>,
    ) -> Result<(), ServerError> {
        println!("xim-create");
        s.set_event_mask(&u.ic, 1, 0)
    }
    fn handle_destroy_ic(
        &mut self,
        _: &mut Im<'_>,
        _: UserInputContext<()>,
    ) -> Result<(), ServerError> {
        Ok(())
    }
    fn handle_reset_ic(
        &mut self,
        _: &mut Im<'_>,
        _: &mut UserInputContext<()>,
    ) -> Result<String, ServerError> {
        Ok(String::new())
    }
    fn handle_set_ic_values(
        &mut self,
        _: &mut Im<'_>,
        _: &mut UserInputContext<()>,
    ) -> Result<(), ServerError> {
        Ok(())
    }
    fn handle_set_focus(
        &mut self,
        _: &mut Im<'_>,
        _: &mut UserInputContext<()>,
    ) -> Result<(), ServerError> {
        println!("xim-focus");
        Ok(())
    }
    fn handle_unset_focus(
        &mut self,
        _: &mut Im<'_>,
        _: &mut UserInputContext<()>,
    ) -> Result<(), ServerError> {
        Ok(())
    }
    fn handle_forward_event(
        &mut self,
        s: &mut Im<'_>,
        u: &mut UserInputContext<()>,
        e: &KeyPressEvent,
    ) -> Result<bool, ServerError> {
        if e.response_type & 127 == 2 && e.detail == 39 {
            s.preedit_draw(&mut u.ic, "拼音")?;
            Ok(true)
        } else if e.response_type & 127 == 2 && e.detail == 36 {
            s.preedit_draw(&mut u.ic, "")?;
            s.commit(&u.ic, "中文输入 ✅")?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
#[test]
#[ignore = "host XIM protocol peer for validate-desktop-interop.py"]
fn host_xim_peer() {
    let (c, n) = x11rb::rust_connection::RustConnection::connect(None).unwrap();
    let mut s = X11rbServer::init(&c, n, "trames-test", xim::ALL_LOCALES).unwrap();
    let mut clients = XimConnections::new();
    let mut h = Handler;
    c.flush().unwrap();
    println!("xim-ready");
    loop {
        let e = c.wait_for_event().unwrap();
        s.filter_event(&e, &mut clients, &mut h).unwrap();
        c.flush().unwrap();
    }
}
