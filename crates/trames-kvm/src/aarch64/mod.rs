// SPDX-License-Identifier: GPL-3.0-or-later
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub(crate) mod engine;
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
