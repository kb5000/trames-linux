// SPDX-License-Identifier: GPL-3.0-or-later
//! Cold vCPU feature discovery. Never infer guest features from host auxv.
use trames_execution::Features;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Registers {
    pub pfr0: u64,
    pub isar0: u64,
    pub isar1: u64,
    pub isar2: u64,
    pub mmfr1: u64,
    pub mmfr2: u64,
}
// These instructions use only the existing GPR/FPSIMD/NZCV/FPCR/FPSR state.
// Stateful SVE/SME/PAuth/MTE/BTI/DIT/SSBS require their complete Linux ABI first.
// Register fields and ELF bit assignments follow Linux arm64 UAPI/cpufeature.
type Selector = fn(Registers) -> u64;
type Rule = (&'static str, usize, u8, Selector, u8, u8);
const RULES: &[Rule] = &[
    ("aes", 0, 3, |r| r.isar0, 4, 1),
    ("pmull", 0, 4, |r| r.isar0, 4, 2),
    ("sha1", 0, 5, |r| r.isar0, 8, 1),
    ("sha2", 0, 6, |r| r.isar0, 12, 1),
    ("crc32", 0, 7, |r| r.isar0, 16, 1),
    ("atomics", 0, 8, |r| r.isar0, 20, 2),
    ("fphp", 0, 9, |r| r.pfr0, 16, 1),
    ("asimdhp", 0, 10, |r| r.pfr0, 20, 1),
    ("asimdrdm", 0, 12, |r| r.isar0, 28, 1),
    ("jscvt", 0, 13, |r| r.isar1, 12, 1),
    ("fcma", 0, 14, |r| r.isar1, 16, 1),
    ("lrcpc", 0, 15, |r| r.isar1, 20, 1),
    ("dcpop", 0, 16, |r| r.isar1, 0, 1),
    ("sha3", 0, 17, |r| r.isar0, 32, 1),
    ("sm3", 0, 18, |r| r.isar0, 36, 1),
    ("sm4", 0, 19, |r| r.isar0, 40, 1),
    ("asimddp", 0, 20, |r| r.isar0, 44, 1),
    ("sha512", 0, 21, |r| r.isar0, 12, 2),
    ("asimdfhm", 0, 23, |r| r.isar0, 48, 1),
    ("uscat", 0, 25, |r| r.mmfr2, 32, 1),
    ("ilrcpc", 0, 26, |r| r.isar1, 20, 2),
    ("flagm", 0, 27, |r| r.isar0, 52, 1),
    ("sb", 0, 29, |r| r.isar1, 36, 1),
    ("dcpodp", 1, 0, |r| r.isar1, 0, 2),
    ("flagm2", 1, 7, |r| r.isar0, 52, 2),
    ("frint", 1, 8, |r| r.isar1, 32, 1),
    ("i8mm", 1, 13, |r| r.isar1, 52, 1),
    ("bf16", 1, 14, |r| r.isar1, 44, 1),
    ("dgh", 1, 15, |r| r.isar1, 48, 1),
    ("rng", 1, 16, |r| r.isar0, 60, 1),
    ("afp", 1, 20, |r| r.mmfr1, 44, 1),
    ("rpres", 1, 21, |r| r.isar2, 4, 1),
    ("ebf16", 1, 32, |r| r.isar1, 44, 2),
    ("cssc", 1, 34, |r| r.isar2, 52, 1),
    ("rprfm", 1, 35, |r| r.isar2, 48, 1),
    ("lse128", 1, 47, |r| r.isar0, 20, 3),
    ("lut", 1, 49, |r| r.isar2, 56, 1),
];
pub(super) fn decode(r: Registers) -> (Features, String) {
    let mut masks = [0; 2];
    let mut names = Vec::new();
    if (r.pfr0 >> 16) & 15 != 15 {
        masks[0] |= 1;
        names.push("fp");
    }
    if (r.pfr0 >> 20) & 15 != 15 {
        masks[0] |= 2;
        names.push("asimd");
    }
    for &(name, word, bit, register, shift, minimum) in RULES {
        let field = (register(r) >> shift) & 15;
        if field >= minimum as u64 && field != 15 {
            masks[word] |= 1 << bit;
            names.push(name);
        }
    }
    (
        Features {
            hwcap: masks[0],
            hwcap2: masks[1],
        },
        names.join(" "),
    )
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
mod native {
    use super::super::machine::system_register;
    use super::*;
    use crate::{CpuCapabilities, Error, Result};
    use kvm_bindings::{
        kvm_vcpu_init, KVM_CAP_ARM_MTE, KVM_CAP_ARM_PTRAUTH_ADDRESS, KVM_CAP_ARM_PTRAUTH_GENERIC,
        KVM_CAP_ARM_SVE, KVM_CAP_SET_GUEST_DEBUG,
    };
    use kvm_ioctls::{Kvm, VcpuFd};
    use std::{collections::BTreeMap, sync::OnceLock};
    static PROFILE: OnceLock<Result<CpuCapabilities>> = OnceLock::new();

    fn registers(cpu: &VcpuFd) -> Result<Registers> {
        let read = |crm, op2| -> Result<u64> {
            let mut bytes = [0; 8];
            cpu.get_one_reg(system_register(3, 0, 0, crm, op2), &mut bytes)
                .map_err(|e| Error::context("read actual vCPU feature ID", e))?;
            Ok(u64::from_ne_bytes(bytes))
        };
        Ok(Registers {
            pfr0: read(4, 0)?,
            isar0: read(6, 0)?,
            isar1: read(6, 1)?,
            isar2: read(6, 2)?,
            mmfr1: read(7, 1)?,
            mmfr2: read(7, 2)?,
        })
    }
    fn probe() -> Result<CpuCapabilities> {
        let kvm = Kvm::new().map_err(|e| Error::context("open feature probe KVM", e))?;
        let vm = kvm
            .create_vm()
            .map_err(|e| Error::context("create feature probe VM", e))?;
        let cpu = vm
            .create_vcpu(0)
            .map_err(|e| Error::context("create feature probe vCPU", e))?;
        let mut target = kvm_vcpu_init::default();
        vm.get_preferred_target(&mut target)
            .map_err(|e| Error::context("feature probe target", e))?;
        cpu.vcpu_init(&target)
            .map_err(|e| Error::context("init feature probe vCPU", e))?;
        let r = registers(&cpu)?;
        let (features, names) = decode(r);
        if features.hwcap & 3 != 3 {
            return Err(Error("KVM execution requires native FP/ASIMD".into()));
        }
        let mut capabilities = BTreeMap::new();
        for (name, cap) in [
            ("sve", KVM_CAP_ARM_SVE),
            ("paca", KVM_CAP_ARM_PTRAUTH_ADDRESS),
            ("pacg", KVM_CAP_ARM_PTRAUTH_GENERIC),
            ("mte", KVM_CAP_ARM_MTE),
            ("guest_debug", KVM_CAP_SET_GUEST_DEBUG),
        ] {
            capabilities.insert(name.to_owned(), kvm.check_extension_raw(cap.into()) > 0);
        }
        Ok(CpuCapabilities {
            architecture: "aarch64",
            hwcap: features.hwcap,
            hwcap2: features.hwcap2,
            features: names,
            kvm_capabilities: capabilities,
            registers: BTreeMap::from([
                ("id_aa64pfr0_el1".into(), r.pfr0),
                ("id_aa64isar0_el1".into(), r.isar0),
                ("id_aa64isar1_el1".into(), r.isar1),
                ("id_aa64isar2_el1".into(), r.isar2),
                ("id_aa64mmfr1_el1".into(), r.mmfr1),
                ("id_aa64mmfr2_el1".into(), r.mmfr2),
            ]),
        })
    }
    pub(crate) fn profile() -> &'static Result<CpuCapabilities> {
        PROFILE.get_or_init(probe)
    }
    /// Each worker's actual CPU must provide the advertised process profile.
    /// This runs only when creating a vCPU, never on a normal continuation.
    pub(crate) fn validate(cpu: &VcpuFd) -> Result<()> {
        let actual = decode(registers(cpu)?).0;
        let expected = profile().as_ref().map_err(|e| Error(e.to_string()))?;
        if actual.hwcap & expected.hwcap != expected.hwcap
            || actual.hwcap2 & expected.hwcap2 != expected.hwcap2
        {
            return Err(Error(
                "worker vCPU does not provide the advertised CPU profile".into(),
            ));
        }
        Ok(())
    }
}
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub(crate) use native::{profile, validate};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn id_fields_never_turn_reserved_or_stateful_extensions_into_guest_promises() {
        let (minimum, names) = decode(Registers::default());
        assert_eq!(
            minimum,
            Features {
                hwcap: 3,
                hwcap2: 0
            }
        );
        assert_eq!(names, "fp asimd");
        let (reserved, names) = decode(Registers {
            pfr0: u64::MAX,
            isar0: u64::MAX,
            isar1: u64::MAX,
            isar2: u64::MAX,
            mmfr1: u64::MAX,
            mmfr2: u64::MAX,
        });
        assert_eq!(reserved, Features::default());
        assert!(names.is_empty());
        let (all, _) = decode(Registers {
            pfr0: 0x1111111111111111,
            isar0: 0x3333333333333333,
            isar1: 0x3333333333333333,
            isar2: 0x3333333333333333,
            mmfr1: 0x3333333333333333,
            mmfr2: 0x3333333333333333,
        });
        assert_eq!(
            all.hwcap
                & ((1 << 2)
                    | (1 << 11)
                    | (1 << 22)
                    | (1 << 24)
                    | (1 << 28)
                    | (1 << 30)
                    | (1 << 31)),
            0
        );
        assert_eq!(
            all.hwcap2 & ((1 << 1) | (1 << 17) | (1 << 18) | (1 << 23)),
            0
        );
    }
    #[test]
    fn real_nested_kvm_ids_expose_native_instruction_families() {
        let (f, names) = decode(Registers {
            pfr0: 0x1101000031110111,
            isar0: 0x0221100110212120,
            isar1: 0x0010221100211002,
            isar2: 0x0100000000000010,
            mmfr1: 0x0000100011312020,
            mmfr2: 0x1001001100000011,
        });
        for name in [
            "aes", "pmull", "sha1", "sha512", "crc32", "atomics", "fphp", "asimdhp", "i8mm",
            "bf16", "lut",
        ] {
            assert!(names.split_whitespace().any(|n| n == name), "{name}");
        }
        assert_ne!(f.hwcap2 & (1 << 32), 0);
        assert_eq!(f.hwcap & (1 << 22), 0);
    }
}
