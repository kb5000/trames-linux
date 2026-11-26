// SPDX-License-Identifier: GPL-3.0-or-later
use crate::Doctor;
use kvm_ioctls::{Cap, Kvm};

pub(crate) fn inspect(mut report: Doctor) -> Doctor {
    // SAFETY: sysconf has no pointer arguments or side effects.
    report.host_page_size = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).ok();
    let kvm = match Kvm::new() {
        Ok(kvm) => kvm,
        Err(error) => {
            report.issues.push(format!("open /dev/kvm: {error}"));
            return report;
        }
    };
    report.kvm_api_version = Some(kvm.get_api_version());
    report.user_memory = kvm.check_extension(Cap::UserMemory);
    report.immediate_exit = kvm.check_extension(Cap::ImmediateExit);
    report.memory_slots = usize::try_from(kvm.check_extension_int(Cap::NrMemslots))
        .ok()
        .filter(|&n| n > 0);
    report.max_vcpus = usize::try_from(kvm.check_extension_int(Cap::MaxVcpus))
        .ok()
        .filter(|&n| n > 0);
    if !report.user_memory {
        report
            .issues
            .push("KVM_CAP_USER_MEMORY is unavailable".into());
    }
    if !report.immediate_exit {
        report
            .issues
            .push("KVM_CAP_IMMEDIATE_EXIT is unavailable".into());
    }
    #[cfg(target_arch = "aarch64")]
    {
        report.arm_ipa_bits = u32::try_from(kvm.check_extension_int(Cap::ArmVmIPASize))
            .ok()
            .filter(|&n| n > 0);
        match crate::aarch64::features::profile() {
            Ok(profile) => report.cpu = Some(profile.clone()),
            Err(error) => report
                .issues
                .push(format!("KVM CPU feature profile: {error}")),
        }
        match kvm.create_vm() {
            Ok(vm) => {
                let attr = kvm_bindings::kvm_device_attr {
                    group: kvm_bindings::KVM_ARM_VM_SMCCC_CTRL,
                    attr: kvm_bindings::KVM_ARM_VM_SMCCC_FILTER as u64,
                    ..Default::default()
                };
                report.smccc_forwarding = vm.has_device_attr(&attr).is_ok();
            }
            Err(error) => report.issues.push(format!("KVM_CREATE_VM: {error}")),
        }
    }
    report
}
