// SPDX-License-Identifier: GPL-3.0-or-later
//! Sparse 4 KiB stage-1 page tables for the L0 fixture, not Linux VMA policy.
pub(super) const PAGE: usize = 4096;
pub(super) const RAM_BASE: u64 = 0x4000_0000;
pub(super) const RAM_SIZE: usize = 16 * PAGE;
pub(super) const VECTOR_VA: u64 = 0x8000;
pub(super) const MAILBOX_VA: u64 = 0xa000;
pub(super) const DOORBELL_VA: u64 = 0xd000;
pub(super) const DOORBELL_GPA: u64 = 0x5000_0000;
pub(super) const CODE_VA: u64 = 0x1_0000;
pub(super) const DATA_VA: u64 = 0x2_0000;
pub(super) const READONLY_VA: u64 = 0x2_1000;
pub(super) const HOLE_VA: u64 = 0x2_2000;
pub(super) const STACK_VA: u64 = 0x3_0000;
pub(super) const VECTOR_PAGE: usize = 4;
pub(super) const MAILBOX_PAGE: usize = 5;
pub(super) const CODE_PAGE: usize = 6;
pub(super) const DATA_PAGE: usize = 7;
pub(super) const READONLY_PAGE: usize = 8;
pub(super) const STACK_PAGE: usize = 9;
const AF: u64 = 1 << 10;
const INNER: u64 = 3 << 8;
const PXN: u64 = 1 << 53;
const UXN: u64 = 1 << 54;
pub(super) const EL1_CODE: u64 = AF | INNER | (2 << 6) | UXN;
pub(super) const EL1_DATA: u64 = AF | INNER | PXN | UXN;
pub(super) const USER_CODE: u64 = AF | INNER | (3 << 6) | PXN;
pub(super) const USER_RW: u64 = AF | INNER | (1 << 6) | PXN | UXN;
pub(super) const USER_RO: u64 = AF | INNER | (3 << 6) | PXN | UXN;

pub(super) fn leaf(gpa: u64, flags: u64) -> Option<u64> {
    (gpa < 1 << 32 && gpa.is_multiple_of(PAGE as u64)).then_some(gpa | flags | 3)
}
pub(super) fn tables() -> [[u64; 512]; 4] {
    let mut result = [[0; 512]; 4];
    for (index, table) in result.iter_mut().enumerate().take(3) {
        table[0] = (RAM_BASE + ((index + 1) * PAGE) as u64) | 3;
    }
    for (va, page, flags) in [
        (VECTOR_VA, VECTOR_PAGE, EL1_CODE),
        (MAILBOX_VA, MAILBOX_PAGE, EL1_DATA),
        (CODE_VA, CODE_PAGE, USER_CODE),
        (DATA_VA, DATA_PAGE, USER_RW),
        (READONLY_VA, READONLY_PAGE, USER_RO),
        (STACK_VA, STACK_PAGE, USER_RW),
    ] {
        result[3][va as usize / PAGE] = leaf(RAM_BASE + (page * PAGE) as u64, flags).unwrap();
    }
    // AttrIndx=1 is device-nGnRE; the stage-2 hole produces the MMIO exit.
    result[3][DOORBELL_VA as usize / PAGE] = leaf(DOORBELL_GPA, AF | (1 << 2) | PXN | UXN).unwrap();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn four_kib_pages_have_independent_permissions_and_a_real_hole() {
        let t = tables();
        let mask = 3 << 6;
        assert_eq!(t[3][DATA_VA as usize / PAGE] & mask, 1 << 6);
        assert_eq!(t[3][READONLY_VA as usize / PAGE] & mask, 3 << 6);
        assert_eq!(t[3][HOLE_VA as usize / PAGE], 0);
        assert_eq!(t[3][MAILBOX_VA as usize / PAGE] & mask, 0);
        assert_ne!(t[3][CODE_VA as usize / PAGE] & PXN, 0);
        assert_ne!(t[3][VECTOR_VA as usize / PAGE] & UXN, 0);
        assert!(!READONLY_VA.is_multiple_of(16384));
    }
    #[test]
    fn high_guest_virtual_addresses_do_not_become_identity_gpa() {
        let t = tables();
        for entry in t[3].iter().filter(|&&entry| entry != 0) {
            let gpa = entry & 0x0000_ffff_ffff_f000;
            assert!(gpa == DOORBELL_GPA || (RAM_BASE..RAM_BASE + RAM_SIZE as u64).contains(&gpa));
        }
        assert_eq!(
            t[3][CODE_VA as usize / PAGE] & 0x0000_ffff_ffff_f000,
            RAM_BASE + (CODE_PAGE * PAGE) as u64
        );
        assert!(leaf(RAM_BASE + 1, USER_RW).is_none());
        assert!(leaf(1 << 32, USER_RW).is_none());
    }
}
