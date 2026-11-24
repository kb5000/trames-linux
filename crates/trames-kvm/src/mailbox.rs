// SPDX-License-Identifier: GPL-3.0-or-later
use crate::Aarch64State;

/// One owner at a time: the running vCPU, or its stopped host worker.
/// No Rust reference into this allocation survives KVM_RUN.
#[repr(C)]
#[derive(Default)]
pub(crate) struct Mailbox {
    pub state: Aarch64State,
    pub syndrome: u64,
    pub address: u64,
    pub event: u64,
    pub transport: u64,
    pub generation: u64,
    pub complete: u64,
    pub invalidate: u64,
}
const _: () = {
    assert!(std::mem::size_of::<Aarch64State>() == 816);
    assert!(std::mem::offset_of!(Mailbox, syndrome) == 816);
    assert!(std::mem::offset_of!(Mailbox, address) == 824);
    assert!(std::mem::offset_of!(Mailbox, event) == 832);
    assert!(std::mem::offset_of!(Mailbox, transport) == 840);
    assert!(std::mem::offset_of!(Mailbox, complete) == 856);
    assert!(std::mem::offset_of!(Mailbox, invalidate) == 864);
    assert!(std::mem::size_of::<Mailbox>() <= 4096);
};
