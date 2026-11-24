// SPDX-License-Identifier: GPL-3.0-or-later
use super::tables::*;
use crate::{
    deadline::Deadline, mailbox::Mailbox, Aarch64State, Doctor, Error, ProbeOptions, ProbeReport,
    Result, Transport,
};
use kvm_bindings::*;
use kvm_ioctls::{Kvm, VcpuFd, VmFd};
use std::{
    marker::PhantomData,
    mem::{align_of, size_of},
    os::fd::AsRawFd,
    ptr::{self, NonNull},
    rc::Rc,
    time::{Duration, Instant},
};

core::arch::global_asm!(include_str!("vectors.S"));
unsafe extern "C" {
    static trames_kvm_vectors_start: u8;
    static trames_kvm_vectors_end: u8;
    static trames_kvm_resume: u8;
    static trames_kvm_guest_start: u8;
    static trames_kvm_guest_end: u8;
    static trames_kvm_memory_start: u8;
    static trames_kvm_memory_end: u8;
    static trames_kvm_loop_start: u8;
    static trames_kvm_loop_end: u8;
}
const FUNCTION_ID: u32 = 0x8600_5452;
// Linux UAPI _IO(KVMIO, 0x80). Only RUN needs a raw wrapper: the reserved
// signal may write immediate_exit, so we never hold a Rust &mut kvm_run while
// it is active. kvm-ioctls owns the mapping and all other ioctl interfaces.
const KVM_RUN_IOCTL: libc::c_ulong = 0xae80;
struct Ram(NonNull<u8>);
impl Ram {
    fn new() -> Result<Self> {
        // SAFETY: fresh demand-zero RAM, never executable in the host.
        let p = unsafe {
            libc::mmap(
                ptr::null_mut(),
                RAM_SIZE,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if p == libc::MAP_FAILED {
            return Err(Error::context(
                "allocate KVM RAM",
                std::io::Error::last_os_error(),
            ));
        }
        Ok(Self(NonNull::new(p.cast()).unwrap()))
    }
    fn pointer(&self, offset: usize) -> *mut u8 {
        assert!(offset < RAM_SIZE);
        // SAFETY: checked offset in our stationary allocation.
        unsafe { self.0.as_ptr().add(offset) }
    }
    fn write<T>(&mut self, offset: usize, value: T) {
        assert!(offset
            .checked_add(size_of::<T>())
            .is_some_and(|end| end <= RAM_SIZE));
        assert!(offset.is_multiple_of(align_of::<T>()));
        // SAFETY: private initialized RAM; every caller owns a stopped vCPU.
        unsafe { self.pointer(offset).cast::<T>().write(value) };
    }
    fn read<T>(&self, offset: usize) -> T {
        assert!(offset
            .checked_add(size_of::<T>())
            .is_some_and(|end| end <= RAM_SIZE));
        assert!(offset.is_multiple_of(align_of::<T>()));
        // SAFETY: T is a plain integer/page-table/mailbox value at every call;
        // no Rust reference or hardware access overlaps this stopped read.
        unsafe { self.pointer(offset).cast::<T>().read() }
    }
    unsafe fn copy_code(&mut self, page: usize, start: *const u8, end: *const u8) -> Result<()> {
        let length = (end as usize)
            .checked_sub(start as usize)
            .filter(|&n| n > 0 && n <= PAGE)
            .ok_or_else(|| Error("adapter/fixture does not fit one 4 KiB page".into()))?;
        // SAFETY: linker symbols bound immutable code; destination is private.
        unsafe { ptr::copy_nonoverlapping(start, self.pointer(page * PAGE), length) };
        Ok(())
    }
    fn publish_code(&self) {
        let mut ctr: u64;
        // SAFETY: architectural user cache publication on owned initialized RAM.
        unsafe {
            core::arch::asm!("mrs {0}, ctr_el0", out(reg) ctr, options(nomem, nostack));
            let dline = 4usize << ((ctr >> 16) & 15);
            let iline = 4usize << (ctr & 15);
            let begin = self.0.as_ptr() as usize;
            for a in (begin..begin + RAM_SIZE).step_by(dline) {
                core::arch::asm!("dc cvau, {0}", in(reg) a, options(nostack));
            }
            core::arch::asm!("dsb ish", options(nostack));
            for a in (begin..begin + RAM_SIZE).step_by(iline) {
                core::arch::asm!("ic ivau, {0}", in(reg) a, options(nostack));
            }
            core::arch::asm!("dsb ish", "isb", options(nostack));
        }
    }
}
impl Drop for Ram {
    fn drop(&mut self) {
        // SAFETY: all vCPU and VM fds are dropped before RAM (field order).
        unsafe {
            libc::munmap(self.0.as_ptr().cast(), RAM_SIZE);
        }
    }
}

/// The engine remains on its creator thread. Declaration order closes the
/// vCPU before the VM, and the VM before its registered RAM can be unmapped.
struct Machine {
    cpu: VcpuFd,
    _vm: VmFd,
    ram: Ram,
    _kvm: Kvm,
    transport: Transport,
    run_calls: u64,
    run: *mut kvm_run,
    _thread_affine: PhantomData<Rc<()>>,
}
fn core_register(offset: usize) -> u64 {
    KVM_REG_ARM64 | KVM_REG_SIZE_U64 | KVM_REG_ARM_CORE as u64 | (offset / 4) as u64
}
fn system_register(op0: u64, op1: u64, crn: u64, crm: u64, op2: u64) -> u64 {
    KVM_REG_ARM64
        | KVM_REG_SIZE_U64
        | KVM_REG_ARM64_SYSREG as u64
        | op0 << 14
        | op1 << 11
        | crn << 7
        | crm << 3
        | op2
}
fn set(cpu: &VcpuFd, id: u64, value: u64) -> Result<()> {
    cpu.set_one_reg(id, &value.to_ne_bytes())
        .map(|_| ())
        .map_err(|e| Error::context(&format!("set KVM register {id:#x}"), e))
}
impl Machine {
    unsafe fn new(transport: Transport, program: (*const u8, *const u8)) -> Result<Self> {
        let kvm = Kvm::new().map_err(|e| Error::context("open KVM", e))?;
        let vm = kvm
            .create_vm()
            .map_err(|e| Error::context("create VM", e))?;
        if transport == Transport::Smccc {
            let filter = kvm_smccc_filter {
                base: FUNCTION_ID,
                nr_functions: 1,
                action: kvm_smccc_filter_action_KVM_SMCCC_FILTER_FWD_TO_USER as u8,
                pad: [0; 15],
            };
            let attr = kvm_device_attr {
                group: KVM_ARM_VM_SMCCC_CTRL,
                attr: KVM_ARM_VM_SMCCC_FILTER as u64,
                addr: &raw const filter as u64,
                flags: 0,
            };
            vm.set_device_attr(&attr)
                .map_err(|e| Error::context("configure SMCCC forwarding", e))?;
        }
        let mut ram = Ram::new()?;
        for (index, table) in tables().into_iter().enumerate() {
            ram.write(index * PAGE, table);
        }
        // SAFETY: immutable linker code is copied once at fixture creation.
        unsafe {
            ram.copy_code(
                VECTOR_PAGE,
                &raw const trames_kvm_vectors_start,
                &raw const trames_kvm_vectors_end,
            )?;
            ram.copy_code(CODE_PAGE, program.0, program.1)?;
        }
        let initial = initial_state();
        ram.write(
            MAILBOX_PAGE * PAGE,
            Mailbox {
                state: initial,
                transport: u64::from(transport == Transport::Smccc),
                invalidate: 3,
                ..Default::default()
            },
        );
        ram.write(DATA_PAGE * PAGE, 0x2345u64);
        ram.publish_code();
        let region = kvm_userspace_memory_region {
            slot: 0,
            guest_phys_addr: RAM_BASE,
            memory_size: RAM_SIZE as u64,
            userspace_addr: ram.0.as_ptr() as u64,
            flags: 0,
        };
        // SAFETY: RAM remains stable and owned through final VM/vCPU close.
        unsafe { vm.set_user_memory_region(region) }
            .map_err(|e| Error::context("register RAM arena", e))?;
        let mut cpu = vm
            .create_vcpu(0)
            .map_err(|e| Error::context("create vCPU", e))?;
        let mut target = kvm_vcpu_init::default();
        vm.get_preferred_target(&mut target)
            .map_err(|e| Error::context("preferred CPU target", e))?;
        cpu.vcpu_init(&target)
            .map_err(|e| Error::context("initialize vCPU", e))?;
        set(&cpu, system_register(3, 0, 2, 0, 0), RAM_BASE)?; // TTBR0_EL1
        set(
            &cpu,
            system_register(3, 0, 2, 0, 2),
            16 | 1 << 8 | 1 << 10 | 3 << 12 | 1 << 23,
        )?; // TCR, 4K/48-bit VA, 32-bit PA
        set(&cpu, system_register(3, 0, 10, 2, 0), 0x04ff)?; // MAIR: normal WB, device-nGnRE
        set(&cpu, system_register(3, 0, 12, 0, 0), VECTOR_VA)?;
        set(&cpu, system_register(3, 0, 1, 0, 2), 3 << 20)?; // FP/SIMD access
        set(
            &cpu,
            core_register(std::mem::offset_of!(kvm_regs, sp_el1)),
            MAILBOX_VA,
        )?;
        let resume = (&raw const trames_kvm_resume as usize)
            - (&raw const trames_kvm_vectors_start as usize);
        set(&cpu, core_register(256), VECTOR_VA + resume as u64)?;
        set(&cpu, core_register(264), 0x3c5)?; // masked EL1h
        set(
            &cpu,
            system_register(3, 0, 1, 0, 0),
            0x30d0_0800 | 1 | 1 << 2 | 1 << 12,
        )?;
        let run = cpu.get_kvm_run() as *mut kvm_run;
        Ok(Self {
            cpu,
            _vm: vm,
            ram,
            _kvm: kvm,
            transport,
            run_calls: 0,
            run,
            _thread_affine: PhantomData,
        })
    }
    /// The caller must drop the guard before this engine and its run mapping.
    unsafe fn deadline(&mut self, duration: Duration) -> Result<Deadline> {
        // SAFETY: retained KvmRun mapping; this expression forms no reference.
        let pointer = unsafe { &raw mut (*self.run).immediate_exit };
        // SAFETY: engine cannot move threads; guard drops before this vCPU.
        unsafe { Deadline::arm(pointer, duration) }
    }
    fn enter(&mut self) -> Result<Option<Mailbox>> {
        loop {
            self.run_calls += 1;
            // SAFETY: creator-thread vCPU; no guest-RAM reference crosses RUN.
            let result = unsafe { libc::ioctl(self.cpu.as_raw_fd(), KVM_RUN_IOCTL as _, 0) };
            let notified = if result == 0 {
                // SAFETY: KVM completed this exit; only immediate_exit can be
                // touched by our signal handler, and no aggregate reference is
                // formed while decoding the disjoint kernel-produced fields.
                let reason = unsafe { (&raw const (*self.run).exit_reason).read_volatile() };
                match reason {
                    KVM_EXIT_HYPERCALL if self.transport == Transport::Smccc => {
                        let nr = unsafe {
                            (&raw const (*self.run).__bindgen_anon_1.hypercall.nr).read_volatile()
                        };
                        require(nr == FUNCTION_ID as u64, "unexpected SMCCC function ID")?;
                        true
                    }
                    KVM_EXIT_MMIO if self.transport == Transport::Mmio => {
                        let mmio = unsafe {
                            (&raw const (*self.run).__bindgen_anon_1.mmio).read_volatile()
                        };
                        require(
                            mmio.phys_addr == DOORBELL_GPA
                                && mmio.is_write == 1
                                && mmio.len == 4
                                && mmio.data[..4] == [1, 0, 0, 0],
                            "unexpected MMIO access",
                        )?;
                        true
                    }
                    KVM_EXIT_INTR => false,
                    _ => return Err(Error(format!("unexpected KVM exit reason {reason}"))),
                }
            } else {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::EINTR) {
                    return Err(Error::context("KVM_RUN", error));
                }
                false
            };
            if notified {
                let mailbox: Mailbox = self.ram.read(MAILBOX_PAGE * PAGE);
                if mailbox.complete != 1 || mailbox.event == 3 {
                    return Err(Error(format!(
                        "EL1 adapter failure: complete={} event={} ESR={:#x} FAR={:#x}",
                        mailbox.complete, mailbox.event, mailbox.syndrome, mailbox.address
                    )));
                }
                return Ok(Some(mailbox));
            }
            if unsafe { (&raw const (*self.run).immediate_exit).read_volatile() } != 0 {
                return Ok(None);
            }
        }
    }
    fn event(&mut self) -> Result<Mailbox> {
        self.enter()?
            .ok_or_else(|| Error("KVM probe deadline expired".into()))
    }
    fn reply(&mut self, mut mailbox: Mailbox, value: u64) {
        mailbox.state.registers[0] = value;
        mailbox.generation += 1;
        self.ram.write(MAILBOX_PAGE * PAGE, mailbox);
    }
}
fn initial_state() -> Aarch64State {
    Aarch64State {
        registers: std::array::from_fn(|index| 0x1000_0000 + index as u64),
        sp: STACK_VA + PAGE as u64,
        pc: CODE_VA,
        nzcv: 0xa000_0000,
        tpidr_el0: 0x1234_5678,
        vectors: std::array::from_fn(|index| {
            0x1122_3344_5566_7788_99aa_bbcc_ddee_0000u128 + index as u128
        }),
        ..Default::default()
    }
}
fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error(message.into()))
    }
}

pub(crate) fn probe(host: Doctor, options: &ProbeOptions) -> Result<ProbeReport> {
    let transport = match options.transport {
        Transport::Auto if host.smccc_forwarding => Transport::Smccc,
        Transport::Auto => Transport::Mmio,
        value => value,
    };
    if transport == Transport::Smccc && !host.smccc_forwarding {
        return Err(Error("SMCCC forwarding is not supported; use mmio".into()));
    }
    let initial = initial_state();
    const WARMUP: u32 = 16;
    let (elapsed, run_calls, preserved) = {
        // SAFETY: these linker symbols bound the immutable fixture.
        let mut machine = unsafe {
            Machine::new(
                transport,
                (
                    &raw const trames_kvm_guest_start,
                    &raw const trames_kvm_guest_end,
                ),
            )
        }?;
        let _deadline = unsafe { machine.deadline(options.deadline) }?;
        let mut begin = Instant::now();
        let mut initial_runs = 0;
        for index in 0..WARMUP + options.iterations {
            if index == WARMUP {
                begin = Instant::now();
                initial_runs = machine.run_calls;
            }
            let m = machine.event()?;
            require(
                m.event == 1 && m.state.registers[8] == 172,
                "original svc did not reach the mailbox",
            )?;
            require(
                m.state.sp == initial.sp
                    && m.state.nzcv == initial.nzcv
                    && m.state.tpidr_el0 == initial.tpidr_el0
                    && m.state.vectors == initial.vectors
                    && m.state.fpcr == 0
                    && m.state.fpsr == 0,
                "SP/NZCV/TLS/FP/SIMD state was not preserved",
            )?;
            for reg in 1..31 {
                if reg != 8 && reg != 9 {
                    require(
                        m.state.registers[reg] == initial.registers[reg],
                        "GPR state was not preserved",
                    )?;
                }
            }
            if index > 0 {
                require(
                    m.state.registers[9] == 0x1000 + index as u64,
                    "mailbox return value did not reach EL0",
                )?;
            }
            machine.reply(m, 0x1000 + index as u64);
        }
        (
            begin.elapsed().as_nanos(),
            machine.run_calls - initial_runs,
            true,
        )
    };
    {
        let mut machine = unsafe {
            Machine::new(
                transport,
                (
                    &raw const trames_kvm_memory_start,
                    &raw const trames_kvm_memory_end,
                ),
            )
        }?;
        let _deadline = unsafe { machine.deadline(options.deadline) }?;
        let m = machine.event()?;
        require(
            m.event == 1 && machine.ram.read::<u64>(DATA_PAGE * PAGE) == 0x2346,
            "guest/host RAM is not canonical",
        )?;
        machine.reply(m, 0);
        let mut fault = machine.event()?;
        require(
            fault.event == 2
                && (fault.syndrome >> 26) & 63 == 0x24
                && fault.address == READONLY_VA
                && fault.syndrome & (1 << 6) != 0,
            "single-page write protection did not fault",
        )?;
        require(
            machine.ram.read::<u64>(READONLY_PAGE * PAGE) == 0,
            "read-only neighbor was modified",
        )?;
        machine.ram.write(
            3 * PAGE + READONLY_VA as usize / PAGE * 8,
            leaf(RAM_BASE + (READONLY_PAGE * PAGE) as u64, USER_RW).unwrap(),
        );
        fault.invalidate = 1;
        machine.reply(fault, 0);
        let m = machine.event()?;
        require(
            m.event == 1
                && machine.ram.read::<u64>(READONLY_PAGE * PAGE) == 0x2346
                && machine.ram.read::<u64>(DATA_PAGE * PAGE) == 0x2346,
            "PTE/TLBI update did not resume the faulting instruction",
        )?;
        machine.reply(m, 0);
        let fault = machine.event()?;
        require(
            fault.event == 2 && (fault.syndrome >> 26) & 63 == 0x24 && fault.address == HOLE_VA,
            "unmapped 4 KiB neighbor did not fault",
        )?;
        let mut next = fault;
        next.state.pc += 4;
        machine.reply(next, 0);
        let fault = machine.event()?;
        require(
            fault.event == 2 && fault.address == MAILBOX_VA && (fault.syndrome >> 26) & 63 == 0x24,
            "EL0 could access the private EL1 mailbox",
        )?;
    }
    let deadline_elapsed = {
        let mut machine = unsafe {
            Machine::new(
                transport,
                (
                    &raw const trames_kvm_loop_start,
                    &raw const trames_kvm_loop_end,
                ),
            )
        }?;
        let delay = options.deadline.min(Duration::from_millis(10));
        let begin = Instant::now();
        let _deadline = unsafe { machine.deadline(delay) }?;
        require(
            machine.enter()?.is_none(),
            "syscall-free loop escaped without a deadline",
        )?;
        begin.elapsed().as_nanos()
    };
    Ok(ProbeReport {
        host,
        transport,
        hardware_executed: true,
        guest_page_size: PAGE,
        ram_slots: 1,
        ram_bytes: RAM_SIZE,
        syscall_round_trips: options.iterations,
        warmup_round_trips: WARMUP,
        kvm_run_calls: run_calls,
        measurement_scope: "SVC+EL1+KVM exit+full-state validation+mailbox reply, excluding warm-up; host environment recorded separately",
        elapsed_ns: elapsed,
        ns_per_round_trip: elapsed as f64 / options.iterations as f64,
        registers_preserved: preserved,
        canonical_memory: true,
        permission_fault: true,
        permission_resume: true,
        unmapped_fault: true,
        el1_isolated: true,
        deadline_interrupted: true,
        deadline_elapsed_ns: deadline_elapsed,
    })
}
