// SPDX-License-Identifier: GPL-3.0-or-later
//! KVM execution transport. Linux semantics belong to the shared runtime.
use serde::Serialize;
use std::{fmt, time::Duration};
pub use trames_execution::Aarch64State;
pub mod kernel;

#[cfg(any(all(target_os = "linux", target_arch = "aarch64"), test))]
mod aarch64;
#[cfg(all(target_os = "linux", any(target_arch = "aarch64", test)))]
mod deadline;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(any(all(target_os = "linux", target_arch = "aarch64"), test))]
mod mailbox;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
mod storage;

#[derive(Debug)]
pub struct Error(pub(crate) String);
impl Error {
    #[cfg(all(target_os = "linux", any(target_arch = "aarch64", test)))]
    pub(crate) fn context(operation: &str, error: impl fmt::Display) -> Self {
        Self(format!("{operation}: {error}"))
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Serialize)]
pub struct Doctor {
    pub host_os: &'static str,
    pub host_arch: &'static str,
    pub host_page_size: Option<usize>,
    pub kvm_api_version: Option<i32>,
    pub user_memory: bool,
    pub immediate_exit: bool,
    pub memory_slots: Option<usize>,
    pub max_vcpus: Option<usize>,
    pub smccc_forwarding: bool,
    pub arm_ipa_bits: Option<u32>,
    pub backend_implemented: bool,
    pub issues: Vec<String>,
}
impl Doctor {
    pub fn inspect() -> Self {
        let report = Self {
            host_os: std::env::consts::OS,
            host_arch: std::env::consts::ARCH,
            host_page_size: None,
            kvm_api_version: None,
            user_memory: false,
            immediate_exit: false,
            memory_slots: None,
            max_vcpus: None,
            smccc_forwarding: false,
            arm_ipa_bits: None,
            backend_implemented: cfg!(all(target_os = "linux", target_arch = "aarch64")),
            issues: Vec::new(),
        };
        #[cfg(target_os = "linux")]
        {
            linux::inspect(report)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let mut report = report;
            report
                .issues
                .push("KVM requires a Linux host; no hardware probe ran".into());
            report
        }
    }
    pub fn require_probe(&self) -> Result<()> {
        if !self.backend_implemented {
            return Err(Error("this bring-up backend requires Linux/aarch64; other guest architectures are not implemented".into()));
        }
        if self.kvm_api_version != Some(12) || !self.user_memory || !self.immediate_exit {
            return Err(Error(format!(
                "required KVM capabilities unavailable: {}",
                self.issues.join("; ")
            )));
        }
        if self.host_page_size != Some(4096) {
            return Err(Error(format!(
                "the L0 hardware probe requires a real 4096-byte host page, found {:?}",
                self.host_page_size
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    Auto,
    Smccc,
    Mmio,
}
impl std::str::FromStr for Transport {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "auto" => Ok(Self::Auto),
            "smccc" => Ok(Self::Smccc),
            "mmio" => Ok(Self::Mmio),
            _ => Err(Error(format!(
                "unknown transport {s:?}; use auto, smccc or mmio"
            ))),
        }
    }
}
#[derive(Clone, Debug)]
pub struct ProbeOptions {
    pub transport: Transport,
    pub iterations: u32,
    pub deadline: Duration,
}
impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            transport: Transport::Auto,
            iterations: 1000,
            deadline: Duration::from_secs(2),
        }
    }
}
impl ProbeOptions {
    pub fn validate(&self) -> Result<()> {
        if !(2..=1_000_000).contains(&self.iterations)
            || self.deadline.is_zero()
            || self.deadline > Duration::from_secs(60)
        {
            return Err(Error(
                "iterations must be 2..=1000000 and deadline must be >0 and <=60s".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Serialize)]
pub struct ProbeReport {
    pub host: Doctor,
    pub transport: Transport,
    pub hardware_executed: bool,
    pub guest_page_size: usize,
    pub ram_slots: usize,
    pub ram_bytes: usize,
    pub syscall_round_trips: u32,
    pub warmup_round_trips: u32,
    pub kvm_run_calls: u64,
    pub measurement_scope: &'static str,
    pub elapsed_ns: u128,
    pub ns_per_round_trip: f64,
    pub registers_preserved: bool,
    pub canonical_memory: bool,
    pub permission_fault: bool,
    pub permission_resume: bool,
    pub unmapped_fault: bool,
    pub el1_isolated: bool,
    pub deadline_interrupted: bool,
    pub deadline_elapsed_ns: u128,
}
pub fn probe(options: &ProbeOptions) -> Result<ProbeReport> {
    options.validate()?;
    let host = Doctor::inspect();
    host.require_probe()?;
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        aarch64::probe(host, options)
    }
    #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
    {
        Err(Error("unsupported execution host".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_hosts_cannot_report_a_hardware_pass() {
        if !cfg!(all(target_os = "linux", target_arch = "aarch64")) {
            assert!(Doctor::inspect().require_probe().is_err());
            assert!(probe(&ProbeOptions::default()).is_err());
        }
    }
    #[test]
    fn user_inputs_are_bounded_and_unknown_transports_fail() {
        assert!("kvm".parse::<Transport>().is_err());
        for iterations in [0, 1, 1_000_001] {
            assert!(ProbeOptions {
                iterations,
                ..Default::default()
            }
            .validate()
            .is_err());
        }
        assert!(ProbeOptions {
            deadline: Duration::ZERO,
            ..Default::default()
        }
        .validate()
        .is_err());
    }
}
