// SPDX-License-Identifier: GPL-3.0-or-later
//! Worker-local vCPU and sparse 4KiB stage-1 tables, with canonical runtime RAM.
use super::machine::{core_register, set, system_register};
use crate::{
    deadline::Deadline,
    mailbox::Mailbox,
    storage::{Allocation, Vm, PAGE},
    Aarch64State, Error, Result,
};
use kvm_bindings::*;
use kvm_ioctls::VcpuFd;
use std::{
    collections::BTreeMap,
    marker::PhantomData,
    os::fd::AsRawFd,
    ptr,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use trames_execution::{ExecutionExit, ExitReason, InstructionCount};
use trames_memory::{
    parallel::View, Access, CpuMemory, HardwarePageInfo, HardwareStorage, HardwareWait, MemoryError,
};

const VECTOR: u64 = 0xffff_ffff_ffff_8000;
const MAILBOX: u64 = 0xffff_ffff_ffff_a000;
const DOORBELL: u64 = 0xffff_ffff_ffff_d000;
const DOORBELL_GPA: u64 = 0x1000_0000;
const FUNCTION: u64 = 0x8600_5452;
const PXN: u64 = 1 << 53;
const UXN: u64 = 1 << 54;
const NORMAL: u64 = (1 << 10) | (3 << 8);
unsafe extern "C" {
    static trames_engine_vectors_start: u8;
    static trames_engine_vectors_end: u8;
    static trames_engine_resume: u8;
    static trames_engine_saving: u8;
    static trames_engine_hvc_return: u8;
    static trames_engine_mmio_return: u8;
}
// Architecture-owned VM profile; generic RAM ownership contains no ARM
// register/SMCCC constants. Future guest architectures supply their own setup.
pub(crate) fn configure_vm(
    fd: &kvm_ioctls::VmFd,
    requested: crate::Transport,
) -> Result<(bool, u64)> {
    let attr = kvm_device_attr {
        group: KVM_ARM_VM_SMCCC_CTRL,
        attr: KVM_ARM_VM_SMCCC_FILTER as u64,
        ..Default::default()
    };
    let available = fd.has_device_attr(&attr).is_ok();
    if requested == crate::Transport::Smccc && !available {
        return Err(Error("requested SMCCC transport is unavailable".into()));
    }
    let smccc = available && requested != crate::Transport::Mmio;
    if smccc {
        let filter = kvm_smccc_filter {
            base: FUNCTION as u32,
            nr_functions: 1,
            action: kvm_smccc_filter_action_KVM_SMCCC_FILTER_FWD_TO_USER as u8,
            pad: [0; 15],
        };
        fd.set_device_attr(&kvm_device_attr {
            addr: &raw const filter as u64,
            ..attr
        })
        .map_err(|e| Error::context("SMCCC filter", e))?;
    }
    Ok((smccc, 1 << 40))
}
fn offset(symbol: *const u8) -> u64 {
    symbol as u64 - &raw const trames_engine_vectors_start as u64
}
fn read<T>(a: &Allocation, offset: usize) -> T {
    assert!(offset + size_of::<T>() <= trames_memory::HardwareRange::len(a));
    // SAFETY: private control storage and stopped creator-thread vCPU. Callers
    // only use plain initialized integer/mailbox values, with no escaping ref.
    unsafe { a.pointer().add(offset).cast::<T>().read() }
}
fn write<T>(a: &Allocation, offset: usize, value: T) {
    assert!(offset + size_of::<T>() <= trames_memory::HardwareRange::len(a));
    unsafe {
        a.pointer().add(offset).cast::<T>().write(value);
    }
}
fn get(cpu: &VcpuFd, id: u64) -> Result<u64> {
    let mut bytes = [0; 8];
    cpu.get_one_reg(id, &mut bytes)
        .map_err(|e| Error::context("read stopped KVM state", e))?;
    Ok(u64::from_ne_bytes(bytes))
}
struct Tables {
    root: Arc<Allocation>,
    children: BTreeMap<(u8, u64), Arc<Allocation>>,
}
impl Tables {
    fn new(vm: &Arc<Vm>) -> Result<Self> {
        Ok(Self {
            root: vm.allocate(PAGE)?,
            children: BTreeMap::new(),
        })
    }
    fn pte(&mut self, vm: &Arc<Vm>, va: u64, descriptor: u64) -> Result<()> {
        let mut table = self.root.clone();
        let mut path = Vec::with_capacity(3);
        for (level, shift) in [39, 30, 21].into_iter().enumerate() {
            let key = (level as u8, va >> shift);
            let child = if let Some(a) = self.children.get(&key) {
                a.clone()
            } else {
                if descriptor == 0 {
                    return Ok(());
                }
                let a = vm.allocate(PAGE)?;
                write(&table, ((va >> shift) & 511) as usize * 8, a.gpa() | 3);
                self.children.insert(key, a.clone());
                a
            };
            path.push((table, key, ((va >> shift) & 511) as usize * 8));
            table = child;
        }
        write(&table, ((va >> 12) & 511) as usize * 8, descriptor);
        if descriptor == 0 {
            for (parent, key, index) in path.into_iter().rev() {
                if (0..512).any(|i| read::<u64>(&table, i * 8) != 0) {
                    break;
                }
                write(&parent, index, 0u64);
                self.children.remove(&key);
                table = parent;
            }
        }
        Ok(())
    }
}
struct Page {
    _owner: Arc<dyn HardwareStorage>,
    info: HardwarePageInfo,
}
#[derive(Clone, Copy)]
pub(crate) enum Event {
    Mailbox,
    Interrupted,
    Lease,
}

pub(crate) struct Executor {
    // Field order closes vCPU before private controls/canonical owners and VM.
    pub(crate) cpu: std::mem::ManuallyDrop<VcpuFd>,
    tables: Tables,
    _upper: Tables,
    mailbox: Arc<Allocation>,
    _vectors: Arc<Allocation>,
    pages: BTreeMap<u64, Page>,
    vm: Arc<Vm>,
    id: u64,
    run: *mut kvm_run,
    interrupt: Arc<crate::deadline::Interrupt>,
    memory_id: u64,
    version: Option<(u64, u64)>,
    invalidate: u64,
    wait: Option<Arc<HardwareWait>>,
    ready: bool,
    run_calls: u64,
    resume_resets: u64,
    _local: PhantomData<Rc<()>>,
}
impl Executor {
    pub fn new() -> Result<Self> {
        let vm = Vm::shared()?;
        let tables = Tables::new(&vm)?;
        let mut upper = Tables::new(&vm)?;
        let vectors = vm.allocate(PAGE)?;
        let mailbox = vm.allocate(PAGE)?;
        let length = offset(&raw const trames_engine_vectors_end) as usize;
        if length > PAGE {
            return Err(Error("EL1 adapter exceeds 4KiB".into()));
        }
        unsafe {
            ptr::copy_nonoverlapping(
                &raw const trames_engine_vectors_start,
                vectors.pointer(),
                length,
            );
        }
        vectors.publish();
        upper.pte(&vm, VECTOR, vectors.gpa() | NORMAL | (2 << 6) | UXN | 3)?;
        upper.pte(&vm, MAILBOX, mailbox.gpa() | NORMAL | PXN | UXN | 3)?;
        upper.pte(
            &vm,
            DOORBELL,
            DOORBELL_GPA | (1 << 10) | (1 << 2) | PXN | UXN | 3,
        )?;
        let id = vm.cpu_id()?;
        let result = (|| {
            let mut cpu = vm
                .fd
                .create_vcpu(id)
                .map_err(|e| Error::context("create worker vCPU", e))?;
            let mut target = kvm_vcpu_init::default();
            vm.fd
                .get_preferred_target(&mut target)
                .map_err(|e| Error::context("vCPU target", e))?;
            cpu.vcpu_init(&target)
                .map_err(|e| Error::context("init worker vCPU", e))?;
            set(&cpu, system_register(3, 0, 2, 0, 0), tables.root.gpa())?;
            set(&cpu, system_register(3, 0, 2, 0, 1), upper.root.gpa())?;
            // 48-bit lower/upper VA, independent 4KiB granules, 40-bit IPA.
            set(
                &cpu,
                system_register(3, 0, 2, 0, 2),
                16 | 1 << 8
                    | 1 << 10
                    | 3 << 12
                    | 16 << 16
                    | 1 << 24
                    | 1 << 26
                    | 3 << 28
                    | 2 << 30
                    | 2 << 32
                    | 1 << 37,
            )?;
            set(&cpu, system_register(3, 0, 10, 2, 0), 0x04ff)?;
            set(&cpu, system_register(3, 0, 12, 0, 0), VECTOR)?;
            set(&cpu, system_register(3, 0, 1, 0, 2), 3 << 20)?;
            set(&cpu, system_register(3, 0, 14, 1, 0), 3)?; // EL0 physical/virtual count reads
            set(
                &cpu,
                core_register(std::mem::offset_of!(kvm_regs, sp_el1)),
                MAILBOX,
            )?;
            set(
                &cpu,
                system_register(3, 0, 1, 0, 0),
                0x30d0_0800 | 1 | 1 << 2 | 1 << 12,
            )?;
            let run = cpu.get_kvm_run() as *mut kvm_run;
            Ok((cpu, run))
        })();
        let (cpu, run) = match result {
            Ok(v) => v,
            Err(e) => {
                vm.release_cpu(id);
                return Err(e);
            }
        };
        let interrupt = match crate::deadline::Interrupt::new() {
            Ok(handle) => handle,
            Err(error) => {
                drop(cpu);
                vm.release_cpu(id);
                return Err(error);
            }
        };
        Ok(Self {
            cpu: std::mem::ManuallyDrop::new(cpu),
            tables,
            _upper: upper,
            mailbox,
            _vectors: vectors,
            pages: BTreeMap::new(),
            vm,
            id,
            run,
            interrupt,
            memory_id: 0,
            version: None,
            invalidate: 3,
            wait: None,
            ready: false,
            run_calls: 0,
            resume_resets: 0,
            _local: PhantomData,
        })
    }
    pub fn interrupt(&self) -> Arc<dyn trames_execution::ExecutionInterrupt> {
        self.interrupt.clone()
    }
    pub fn take_wait(&mut self) -> Option<Arc<HardwareWait>> {
        self.wait.take()
    }
    pub fn mapped_bytes(&self) -> usize {
        self.pages.len() * PAGE
    }
    pub fn slot_count(&self) -> usize {
        self.vm.slot_count()
    }
    fn raw_run(&mut self) -> std::io::Result<()> {
        self.run_calls += 1;
        let r = unsafe { libc::ioctl(self.cpu.as_raw_fd(), 0xae80 as libc::c_ulong as _, 0) };
        if r == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    pub(crate) fn event(&mut self) -> Result<Event> {
        if self.interrupt.take_request() {
            unsafe {
                (&raw mut (*self.run).immediate_exit).write_volatile(1);
            }
        }
        self.ready = false;
        match self.raw_run() {
            Err(e) if e.raw_os_error() == Some(libc::EINTR) => return Ok(Event::Interrupted),
            Err(e) if e.raw_os_error() == Some(libc::EFAULT) => return Ok(Event::Lease),
            Err(e) => return Err(Error::context("worker KVM_RUN", e)),
            Ok(()) => {}
        }
        let reason = unsafe { (&raw const (*self.run).exit_reason).read_volatile() };
        let valid = match reason {
            KVM_EXIT_HYPERCALL if self.vm.smccc => unsafe {
                (&raw const (*self.run).__bindgen_anon_1.hypercall.nr).read_volatile() == FUNCTION
            },
            KVM_EXIT_MMIO if !self.vm.smccc => {
                let m = unsafe { (&raw const (*self.run).__bindgen_anon_1.mmio).read_volatile() };
                m.phys_addr == DOORBELL_GPA
                    && m.is_write == 1
                    && m.len == 4
                    && m.data[..4] == [1, 0, 0, 0]
            }
            KVM_EXIT_INTR => return Ok(Event::Interrupted),
            _ => false,
        };
        if !valid {
            return Err(Error(format!("unexpected worker KVM exit {reason}")));
        }
        let m: Mailbox = read(&self.mailbox, 0);
        if m.complete != 1 || m.event == 3 {
            return Err(Error(format!(
                "private EL1 fault: event={} ESR={:#x} FAR={:#x}",
                m.event, m.syndrome, m.address
            )));
        }
        // ARM64 SMCCC forwarding already completed HVC; its kernel run path
        // has no userspace hypercall completion callback. MMIO still must be
        // completed before the task can park/migrate. Keep that extra RUN only
        // on the compatibility transport, with immediate_exit preventing EL0.
        if !self.vm.smccc {
            unsafe {
                (&raw mut (*self.run).immediate_exit).write_volatile(1);
            }
            match self.raw_run() {
                Err(e) if e.raw_os_error() == Some(libc::EINTR) => {}
                other => return Err(Error(format!("KVM pending exit completion: {other:?}"))),
            }
            unsafe {
                (&raw mut (*self.run).immediate_exit).write_volatile(0);
            }
        }
        self.ready = true;
        Ok(Event::Mailbox)
    }
    fn start(&mut self, state: &Aarch64State) -> Result<()> {
        write(
            &self.mailbox,
            0,
            Mailbox {
                state: state.clone(),
                transport: u64::from(self.vm.smccc),
                invalidate: std::mem::take(&mut self.invalidate),
                ..Default::default()
            },
        );
        // A completed notification is already at the branch to full restore.
        // Updating its mailbox handles either the same task or a new assignment
        // without register ioctls on normal syscall/fault paths. An asynchronous
        // exit may leave partial restore/EL0 state and needs a cold reset.
        if !self.ready {
            self.resume_resets += 1;
            set(
                &self.cpu,
                core_register(std::mem::offset_of!(kvm_regs, regs.pc)),
                VECTOR + offset(&raw const trames_engine_resume),
            )?;
            set(
                &self.cpu,
                core_register(std::mem::offset_of!(kvm_regs, regs.pstate)),
                0x3c5,
            )?;
        }
        Ok(())
    }

    fn flags(info: HardwarePageInfo) -> u64 {
        let mut f = NORMAL | PXN | 3 | (1 << 6);
        if !info.writable {
            f |= 1 << 7;
        }
        if !info.permissions.execute {
            f |= UXN;
        }
        f
    }
    fn synchronize(&mut self, memory: &View) -> Result<()> {
        let id = memory.instruction_identity().0;
        let version = memory.hardware_view_version();
        if self.memory_id != id {
            let tables = Tables::new(&self.vm)?;
            set(&self.cpu, system_register(3, 0, 2, 0, 0), tables.root.gpa())?;
            self.tables = tables;
            self.pages.clear();
            self.memory_id = id;
            self.version = None;
            self.invalidate |= 1;
        }
        if self.version.map(|v| v.0) != Some(version.0) {
            let changed: Vec<_> = self
                .pages
                .keys()
                .copied()
                .filter(|&va| match memory.hardware_page_info(va) {
                    Ok(info) => {
                        let old = &self.pages[&va].info;
                        info.storage_id != old.storage_id
                            || info.writable != old.writable
                            || info.permissions != old.permissions
                            || info.backing_version != old.backing_version
                            || info.instruction_revision != old.instruction_revision
                    }
                    Err(_) => true,
                })
                .collect();
            // Revoke old leaves before dropping their physical owners. Only this
            // stopped worker can use its tables; TLBI precedes the next EL0 entry.
            for va in changed {
                self.tables.pte(&self.vm, va, 0)?;
                self.pages.remove(&va);
                self.invalidate |= 1;
            }
        }
        self.version = Some(version);
        Ok(())
    }
    fn fault(&mut self, memory: &View, m: &Mailbox) -> std::result::Result<(), super::EngineError> {
        let ec = (m.syndrome >> 26) & 63;
        if ec != 0x20 && ec != 0x24 {
            return Err(super::EngineError::Trap {
                syndrome: m.syndrome,
                address: m.address,
            });
        }
        if m.syndrome & 63 == 0x21 {
            return Err(MemoryError::Alignment(m.address).into());
        }
        let status = m.syndrome & 63;
        if status <= 3 {
            return Err(MemoryError::AddressRange(m.address).into());
        }
        if !(4..=15).contains(&status) {
            return Err(super::EngineError::Trap {
                syndrome: m.syndrome,
                address: m.address,
            });
        }
        let access = if ec == 0x20 {
            Access::Execute
        } else if m.syndrome & (1 << 6) != 0 && m.syndrome & (1 << 8) == 0 {
            Access::Write
        } else {
            Access::Read
        };
        memory.check(m.address, 1, access)?;
        let va = m.address & 0x0000_ffff_ffff_f000;
        let vm = &self.vm;
        if access != Access::Execute && memory.hardware_ranges_enabled() {
            // SAFETY: all views use runtime hardware ownership, and the range
            // allocator supplies stable fresh zero bytes under the same gate.
            unsafe { memory.export_hardware_range(va, || vm.range()) }?;
        }
        let (owner, info) = unsafe {
            memory.export_hardware_page_with_mapping(
                va,
                access,
                |bytes| vm.page(bytes),
                |mapping| vm.import(mapping),
            )
        }?;
        if !info.permissions.read {
            return Err(MemoryError::Permission {
                address: m.address,
                access: Access::Read,
            }
            .into());
        }
        if info.permissions.execute {
            self.vm
                .publish_code(owner.physical_address(), owner.pointer())?;
        }
        self.tables
            .pte(&self.vm, va, owner.physical_address() | Self::flags(info))?;
        self.pages.insert(
            va,
            Page {
                _owner: owner,
                info,
            },
        );
        self.invalidate |= if info.permissions.execute { 3 } else { 1 };
        Ok(())
    }
    fn capture_el0(&self) -> Result<Aarch64State> {
        let mut state = Aarch64State::default();
        for (i, r) in state.registers.iter_mut().enumerate() {
            *r = get(&self.cpu, core_register(i * 8))?;
        }
        state.pc = get(
            &self.cpu,
            core_register(std::mem::offset_of!(kvm_regs, regs.pc)),
        )?;
        state.sp = get(
            &self.cpu,
            core_register(std::mem::offset_of!(kvm_regs, regs.sp)),
        )?;
        state.nzcv = (get(
            &self.cpu,
            core_register(std::mem::offset_of!(kvm_regs, regs.pstate)),
        )? as u32)
            & 0xf0000000;
        state.tpidr_el0 = get(&self.cpu, system_register(3, 3, 13, 0, 2))?;
        for (index, vector) in state.vectors.iter_mut().enumerate() {
            let id = KVM_REG_ARM64
                | KVM_REG_SIZE_U128
                | KVM_REG_ARM_CORE as u64
                | ((std::mem::offset_of!(kvm_regs, fp_regs.vregs) + index * 16) / 4) as u64;
            let mut bytes = [0; 16];
            self.cpu
                .get_one_reg(id, &mut bytes)
                .map_err(|e| Error::context("capture SIMD", e))?;
            *vector = u128::from_ne_bytes(bytes);
        }
        for (target, off) in [
            (
                &mut state.fpcr,
                std::mem::offset_of!(kvm_regs, fp_regs.fpcr),
            ),
            (
                &mut state.fpsr,
                std::mem::offset_of!(kvm_regs, fp_regs.fpsr),
            ),
        ] {
            let id = KVM_REG_ARM64 | KVM_REG_SIZE_U32 | KVM_REG_ARM_CORE as u64 | (off / 4) as u64;
            let mut bytes = [0; 4];
            self.cpu
                .get_one_reg(id, &mut bytes)
                .map_err(|e| Error::context("capture FP control", e))?;
            *target = u32::from_ne_bytes(bytes);
        }
        Ok(state)
    }
    // A stopped EL0 bank is complete. In restore, the complete mailbox still
    // owns the original user state. In save/vector, finish bounded private code
    // until its notification so a pending SVC/fault is never lost.
    fn checkpoint(&mut self, state: &mut Aarch64State) -> Result<Option<Mailbox>> {
        let pstate = get(
            &self.cpu,
            core_register(std::mem::offset_of!(kvm_regs, regs.pstate)),
        )?;
        if pstate & 15 == 0 {
            *state = self.capture_el0()?;
            return Ok(None);
        }
        let pc = get(
            &self.cpu,
            core_register(std::mem::offset_of!(kvm_regs, regs.pc)),
        )?;
        if pc == VECTOR + offset(&raw const trames_engine_hvc_return)
            || pc == VECTOR + offset(&raw const trames_engine_mmio_return)
            || (VECTOR + offset(&raw const trames_engine_resume)
                ..VECTOR + offset(&raw const trames_engine_saving))
                .contains(&pc)
        {
            *state = read::<Mailbox>(&self.mailbox, 0).state;
            return Ok(None);
        }
        if !(VECTOR..VECTOR + PAGE as u64).contains(&pc) {
            return Err(Error(format!("unexpected privileged PC {pc:#x}")));
        }
        self.interrupt.take_request();
        let begin = Instant::now();
        let _finish = unsafe {
            Deadline::arm(
                &raw mut (*self.run).immediate_exit,
                Duration::from_millis(50),
            )
        }?;
        for _ in 0..16 {
            match self.event()? {
                Event::Mailbox => {
                    let m: Mailbox = read(&self.mailbox, 0);
                    *state = m.state.clone();
                    return Ok(Some(m));
                }
                Event::Interrupted if begin.elapsed() < Duration::from_millis(50) => unsafe {
                    (&raw mut (*self.run).immediate_exit).write_volatile(0);
                },
                _ => break,
            }
        }
        Err(Error(
            "EL1 save could not reach a bounded checkpoint".into(),
        ))
    }
    pub fn run(
        &mut self,
        state: &mut Aarch64State,
        memory: &mut View,
        quantum: Duration,
    ) -> std::result::Result<ExecutionExit, super::EngineError> {
        self.wait = None;
        if state.tpidr2_el0 != 0 {
            return Err(Error("SME context is not supported by KVM baseline".into()).into());
        }
        self.synchronize(memory)?;
        self.start(state)?;
        let begin = Instant::now();
        let mut deadline =
            Some(unsafe { Deadline::arm(&raw mut (*self.run).immediate_exit, quantum) }?);
        if self.interrupt.take_request() {
            unsafe {
                (&raw mut (*self.run).immediate_exit).write_volatile(1);
            }
        }
        let mut lease_retry = None;
        let result = (|| loop {
            let event = self.event()?;
            let mailbox = match event {
                Event::Mailbox => {
                    let m: Mailbox = read(&self.mailbox, 0);
                    *state = m.state.clone();
                    m
                }
                Event::Interrupted | Event::Lease => {
                    // Drop the old timer before a bounded save completion can
                    // arm its own timer; all guards precede vCPU destruction.
                    drop(deadline.take());
                    let m = self.checkpoint(state)?;
                    if let Some(m) = m {
                        m
                    } else {
                        if matches!(event, Event::Lease) {
                            let (wait, generation) = self.vm.wait_for_lease();
                            if let Some(wait) = wait {
                                self.wait = Some(wait);
                            } else if lease_retry != Some(generation) && begin.elapsed() < quantum {
                                lease_retry = Some(generation);
                                self.start(state)?;
                                deadline = Some(unsafe {
                                    Deadline::arm(
                                        &raw mut (*self.run).immediate_exit,
                                        quantum
                                            .saturating_sub(begin.elapsed())
                                            .max(Duration::from_nanos(1)),
                                    )
                                }?);
                                continue;
                            } else if lease_retry == Some(generation) {
                                return Err(Error(
                                    "KVM EFAULT without an active byte lease".into(),
                                )
                                .into());
                            }
                        }
                        return Ok(ExecutionExit {
                            reason: ExitReason::Yield,
                            instructions: InstructionCount::Unavailable,
                        });
                    }
                }
            };
            if mailbox.event == 1 {
                return Ok(ExecutionExit {
                    reason: ExitReason::Syscall,
                    instructions: InstructionCount::Unavailable,
                });
            }
            self.fault(memory, &mailbox)?;
            drop(deadline.take());
            if begin.elapsed() >= quantum {
                return Ok(ExecutionExit {
                    reason: ExitReason::Yield,
                    instructions: InstructionCount::Unavailable,
                });
            }
            let mut reply = mailbox;
            reply.invalidate = std::mem::take(&mut self.invalidate);
            write(&self.mailbox, 0, reply);
            // If checkpoint had to finish an EL1 save, its original slice timer
            // was removed. Re-arm the remaining duration before fault retry.
            if deadline.is_none() {
                deadline = Some(unsafe {
                    Deadline::arm(
                        &raw mut (*self.run).immediate_exit,
                        quantum
                            .saturating_sub(begin.elapsed())
                            .max(Duration::from_nanos(1)),
                    )
                }?);
            }
        })();
        // Runtime publication happens after hardware stores stop, under its
        // existing dirty-byte/code/accounting rules. Owners remain retained.
        memory.mark_hardware_private_writes();
        for (&va, page) in &self.pages {
            if page.info.writable {
                memory.mark_hardware_write(va)?;
            }
        }
        drop(deadline);
        result
    }
}
impl Drop for Executor {
    fn drop(&mut self) {
        self.interrupt.retire();
        unsafe {
            std::mem::ManuallyDrop::drop(&mut self.cpu);
        }
        self.vm.release_cpu(self.id);
    }
}

#[cfg(test)]
mod hardware_checkpoints {
    use super::*;
    #[test]
    #[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
    fn completed_syscalls_resume_without_register_resets_or_extra_smccc_runs() {
        let mut memory = trames_memory::Memory::default();
        memory
            .map(0x10000, 4096, trames_memory::Permissions::RW)
            .unwrap();
        memory
            .write_array(0x10000, 0xd4000001u32.to_le_bytes())
            .unwrap();
        memory
            .write_array(0x10004, 0x17ffffffu32.to_le_bytes())
            .unwrap();
        memory
            .protect(0x10000, 4096, trames_memory::Permissions::RX)
            .unwrap();
        let mut registry = trames_memory::parallel::Registry::default();
        registry.use_hardware();
        memory.prepare_hardware().unwrap();
        let mut view = memory.parallel_view(&mut registry).unwrap();
        view.enable_hardware();
        let mut executor = Executor::new().unwrap();
        let mut state = Aarch64State {
            pc: 0x10000,
            ..Default::default()
        };
        let quantum = Duration::from_millis(100);
        assert_eq!(
            executor.run(&mut state, &mut view, quantum).unwrap().reason,
            ExitReason::Syscall
        );
        let runs = executor.run_calls;
        let resets = executor.resume_resets;
        for _ in 0..100 {
            assert_eq!(
                executor.run(&mut state, &mut view, quantum).unwrap().reason,
                ExitReason::Syscall
            );
        }
        assert_eq!(
            executor.run_calls - runs,
            if executor.vm.smccc { 100 } else { 200 }
        );
        assert_eq!(executor.resume_resets, resets);
    }
    #[test]
    #[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
    fn sparse_table_retirement_releases_empty_subtrees() {
        let vm = Vm::shared().unwrap();
        let mut tables = Tables::new(&vm).unwrap();
        let leaf = vm.allocate(PAGE).unwrap();
        for va in [0x1000, 0x7fff_1234_5000, 0x1234_5678_9000] {
            tables.pte(&vm, va, leaf.gpa() | NORMAL | 3).unwrap();
        }
        assert_eq!(tables.children.len(), 9);
        for va in [0x1000, 0x7fff_1234_5000, 0x1234_5678_9000] {
            tables.pte(&vm, va, 0).unwrap();
        }
        assert!(tables.children.is_empty());
        assert!((0..512).all(|i| read::<u64>(&tables.root, i * 8) == 0));
    }
    #[test]
    #[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
    fn every_restore_pc_and_completed_doorbell_has_a_complete_user_checkpoint() {
        let mut executor = Executor::new().unwrap();
        let expected = Aarch64State {
            registers: std::array::from_fn(|i| 0x1000 + i as u64),
            pc: 0x1234_5678,
            sp: 0x90000,
            nzcv: 0xa0000000,
            tpidr_el0: 0x12345678,
            vectors: std::array::from_fn(|i| 0x112233445566778899aabbccdd000000u128 + i as u128),
            ..Default::default()
        };
        executor.start(&expected).unwrap();
        let positions = (offset(&raw const trames_engine_resume)
            ..offset(&raw const trames_engine_saving))
            .step_by(4)
            .chain([
                offset(&raw const trames_engine_hvc_return),
                offset(&raw const trames_engine_mmio_return),
            ]);
        for position in positions {
            set(
                &executor.cpu,
                core_register(std::mem::offset_of!(kvm_regs, regs.pc)),
                VECTOR + position,
            )
            .unwrap();
            let mut state = Aarch64State::default();
            assert!(executor.checkpoint(&mut state).unwrap().is_none());
            assert_eq!(state, expected, "PC +{position:#x}");
        }
    }
    #[test]
    #[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
    fn hardware_step_can_pause_inside_a_real_partial_el1_save() {
        let mut executor = Executor::new().unwrap();
        let code = executor.vm.allocate(PAGE).unwrap();
        // Original SVC; loop. Direct private test setup installs only this RX page.
        write(&code, 0, 0xd4000001u32);
        write(&code, 4, 0x14000000u32);
        code.publish();
        executor
            .tables
            .pte(
                &executor.vm,
                0x10000,
                code.gpa() | NORMAL | PXN | (3 << 6) | 3,
            )
            .unwrap();
        let expected = Aarch64State {
            registers: std::array::from_fn(|i| 0x1000 + i as u64),
            pc: 0x10000,
            sp: 0x30000,
            nzcv: 0xa0000000,
            tpidr_el0: 0x12345678,
            vectors: std::array::from_fn(|i| 0x112233445566778899aabbccdd000000u128 + i as u128),
            ..Default::default()
        };
        executor.start(&expected).unwrap();
        executor
            .cpu
            .set_guest_debug(&kvm_guest_debug {
                control: KVM_GUESTDBG_ENABLE | KVM_GUESTDBG_SINGLESTEP,
                ..Default::default()
            })
            .unwrap();
        let mut reached = false;
        for _ in 0..512 {
            let guard = unsafe {
                Deadline::arm(
                    &raw mut (*executor.run).immediate_exit,
                    Duration::from_millis(100),
                )
            }
            .unwrap();
            executor.raw_run().unwrap();
            drop(guard);
            let pc = get(
                &executor.cpu,
                core_register(std::mem::offset_of!(kvm_regs, regs.pc)),
            )
            .unwrap();
            let pstate = get(
                &executor.cpu,
                core_register(std::mem::offset_of!(kvm_regs, regs.pstate)),
            )
            .unwrap();
            if pstate & 15 != 0
                && pc >= VECTOR + offset(&raw const trames_engine_saving) + 12
                && pc < VECTOR + offset(&raw const trames_engine_hvc_return)
            {
                reached = true;
                break;
            }
        }
        executor
            .cpu
            .set_guest_debug(&kvm_guest_debug::default())
            .unwrap();
        assert!(
            reached,
            "hardware single-step must actually enter the EL1 save range"
        );
        let mut state = Aarch64State::default();
        let event = executor
            .checkpoint(&mut state)
            .unwrap()
            .expect("pending SVC must be returned rather than discarded as Yield");
        assert_eq!(event.event, 1);
        let mut expected = expected;
        expected.pc += 4;
        assert_eq!(state, expected);
    }
}
