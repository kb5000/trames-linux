// SPDX-License-Identifier: GPL-3.0-or-later
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
mod machine;
mod tables;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub(crate) use machine::probe;
