// SPDX-License-Identifier: GPL-3.0-or-later
// Match Linux's ordinary EL0 cache access and WFE policy: CTR_EL0, DC ZVA,
// user clean/invalidate and event waits execute with the Linux permissions.
// No instruction rewriting
// or per-instruction userspace emulation. See Linux arm64 INIT_SCTLR_EL1_MMU_ON.
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const USER_SCTLR: u64 =
    0x30d0_0800 | 1 | (1 << 2) | (1 << 12) | (1 << 14) | (1 << 15) | (1 << 18) | (1 << 26);

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub(crate) mod engine;
pub(crate) mod features;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
mod machine;
mod tables;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub(crate) use machine::probe;

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
#[derive(Debug)]
pub(crate) enum EngineError {
    Host(crate::Error),
    Memory(trames_memory::MemoryError),
    Trap { syndrome: u64, address: u64 },
}
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
impl From<crate::Error> for EngineError {
    fn from(value: crate::Error) -> Self {
        Self::Host(value)
    }
}
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
impl From<trames_memory::MemoryError> for EngineError {
    fn from(value: trames_memory::MemoryError) -> Self {
        Self::Memory(value)
    }
}
