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
    sync::{Arc, Weak},
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
// Both EL0 mappings and each executor's private EL1 controls are non-global.
// Cached address spaces must never observe another root's translations.
const NORMAL: u64 = (1 << 10) | (3 << 8) | (1 << 11);
unsafe extern "C" {
    static trames_engine_vectors_start: u8;
    static trames_engine_vectors_end: u8;
    static trames_engine_resume: u8;
    static trames_engine_step_resume: u8;
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
    owner: Weak<dyn HardwareStorage>,
    info: HardwarePageInfo,
}
// Match the existing HVF ownership model: stopped roots retain translation
// metadata and weak physical aliases, never the lifetime of a retired mm/RAM.
const CACHED_SPACES: usize = 32;
const CACHED_PAGES: usize = 32768;
struct AddressSpace {
    tables: Tables,
    pages: BTreeMap<u64, Page>,
    asid: u16,
    used: u64,
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
    asid: u16,
    asid_base: u16,
    asid_count: u16,
    spaces: BTreeMap<u64, AddressSpace>,
    clock: u64,
    version: Option<(u64, u64)>,
    invalidate: u64,
    wait: Option<Arc<HardwareWait>>,
    ready: bool,
    stepping: bool,
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
            DOORBELL_GPA | (1 << 10) | (1 << 11) | (1 << 2) | PXN | UXN | 3,
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
            super::features::validate(&cpu)?;
            let asid_bits = match (get(&cpu, system_register(3, 0, 0, 7, 0))? >> 4) & 15 {
                0 => 8,
                2 => 16,
                value => return Err(Error(format!("unsupported KVM ASID width {value}"))),
            };
            let asid_count = if asid_bits == 16 {
                CACHED_SPACES as u64
            } else {
                8
            };
            let asid_base = id * asid_count;
            if asid_base + asid_count > 1u64 << asid_bits {
                return Err(Error(format!("KVM {asid_bits}-bit ASID banks exhausted")));
            }
            set(
                &cpu,
                system_register(3, 0, 2, 0, 0),
                tables.root.gpa() | (asid_base << 48),
            )?;
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
                    | 1 << 37
                    | u64::from(asid_bits == 16) << 36,
            )?;
            set(&cpu, system_register(3, 0, 10, 2, 0), 0x04ff)?;
            set(&cpu, system_register(3, 0, 12, 0, 0), VECTOR)?;
            set(&cpu, system_register(3, 0, 1, 0, 2), 3 << 20)?;
            set(&cpu, system_register(2, 0, 0, 2, 2), 0)?;
            set(&cpu, system_register(3, 0, 14, 1, 0), 3)?; // EL0 physical/virtual count reads
            set(
                &cpu,
                core_register(std::mem::offset_of!(kvm_regs, sp_el1)),
                MAILBOX,
            )?;
            set(&cpu, system_register(3, 0, 1, 0, 0), super::USER_SCTLR)?;
            let run = cpu.get_kvm_run() as *mut kvm_run;
            Ok((cpu, run, asid_base as u16, asid_count as u16))
        })();
        let (cpu, run, asid_base, asid_count) = match result {
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
            asid: asid_base,
            asid_base,
            asid_count,
            spaces: BTreeMap::new(),
            clock: 0,
            version: None,
            invalidate: 3,
            wait: None,
            ready: false,
            stepping: false,
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
        self.pages
            .values()
            .chain(self.spaces.values().flat_map(|s| s.pages.values()))
            .filter(|page| page.owner.strong_count() != 0)
            .count()
            * PAGE
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
                context: u64::from(self.asid) << 48,
                invalidate: std::mem::take(&mut self.invalidate),
                ..Default::default()
            },
        );
        // A completed notification is already at the branch to full restore.
        // Updating its mailbox handles either the same task or a new assignment
        // without register ioctls on normal syscall/fault paths. An asynchronous
        // exit may leave partial restore/EL0 state and needs a cold reset.
        let stepping = state.nzcv & (1 << 21) != 0;
        if !self.ready || stepping || self.stepping {
            if self.stepping != stepping {
                // Debug admission is task-local. Change the vCPU register only
                // at the actual mode transition; ordinary SVCs add no ioctl.
                set(
                    &self.cpu,
                    system_register(2, 0, 0, 2, 2),
                    if stepping { 0x8001 } else { 0 },
                )?;
                self.stepping = stepping;
            }
            self.reset_resume(stepping)?;
        }
        Ok(())
    }
    fn reset_resume(&mut self, stepping: bool) -> Result<()> {
        self.resume_resets += 1;
        set(
            &self.cpu,
            core_register(std::mem::offset_of!(kvm_regs, regs.pc)),
            VECTOR
                + offset(if stepping {
                    &raw const trames_engine_step_resume
                } else {
                    &raw const trames_engine_resume
                }),
        )?;
        set(
            &self.cpu,
            core_register(std::mem::offset_of!(kvm_regs, regs.pstate)),
            0x3c5,
        )
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
            let next = match self.spaces.remove(&id) {
                Some(space) => space,
                None => {
                    // Each worker owns a disjoint bank. Reuse never changes its
                    // private mailbox/vector physical identity, so the restore
                    // can read maintenance flags before invalidating that ASID.
                    let asid = if self.memory_id == 0 {
                        self.asid
                    } else if let Some(asid) = (0..self.asid_count)
                        .map(|i| self.asid_base + i)
                        .find(|&asid| {
                            asid != self.asid
                                && !self.spaces.values().any(|space| space.asid == asid)
                        })
                    {
                        asid
                    } else {
                        let oldest = *self
                            .spaces
                            .iter()
                            .min_by_key(|(_, space)| space.used)
                            .expect("ASID bank has inactive roots")
                            .0;
                        self.spaces.remove(&oldest).unwrap().asid
                    };
                    // A retired root may have used this ASID on any host CPU.
                    // Architectural maintenance completes before any EL0 entry.
                    self.invalidate |= 3;
                    AddressSpace {
                        tables: Tables::new(&self.vm)?,
                        pages: BTreeMap::new(),
                        asid,
                        used: 0,
                    }
                }
            };
            set(
                &self.cpu,
                system_register(3, 0, 2, 0, 0),
                next.tables.root.gpa() | (u64::from(next.asid) << 48),
            )?;
            self.clock = self.clock.wrapping_add(1);
            let old = AddressSpace {
                tables: std::mem::replace(&mut self.tables, next.tables),
                pages: std::mem::replace(&mut self.pages, next.pages),
                asid: std::mem::replace(&mut self.asid, next.asid),
                used: self.clock,
            };
            if self.memory_id != 0 {
                self.spaces.insert(self.memory_id, old);
            }
            self.memory_id = id;
            // Retained weak aliases are always revalidated before selecting a
            // stopped root, including owners retired while another mm ran.
            self.version = None;
            self.invalidate |= 1;
            while self.spaces.len() + 1 > CACHED_SPACES
                || self.pages.len() + self.spaces.values().map(|s| s.pages.len()).sum::<usize>()
                    > CACHED_PAGES
            {
                let Some((&oldest, _)) = self.spaces.iter().min_by_key(|(_, space)| space.used)
                else {
                    break;
                };
                self.spaces.remove(&oldest);
            }
        }
        if self.version.map(|v| v.0) != Some(version.0) {
            let changed: Vec<_> = self
                .pages
                .keys()
                .copied()
                .filter(|&va| match memory.hardware_page_info(va) {
                    Ok(info) => {
                        let old = &self.pages[&va].info;
                        self.pages[&va].owner.strong_count() == 0
                            || info.storage_id != old.storage_id
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
                owner: Arc::downgrade(&owner),
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
            let mailbox: Mailbox = read(&self.mailbox, 0);
            if mailbox.complete == 1 && mailbox.event == 3 {
                return Err(Error(format!(
                    "private EL1 fault: event={} ESR={:#x} FAR={:#x}",
                    mailbox.event, mailbox.syndrome, mailbox.address
                )));
            }
            *state = mailbox.state.clone();
            // start() transfers maintenance ownership to this mailbox. A kick
            // may stop restore before TLBI (or before its acknowledgement).
            // Carry unacknowledged flags into the next assignment; otherwise a
            // fresh mailbox would lose a completed mprotect/unmap revocation.
            self.invalidate |= mailbox.invalidate;
            // A deadline/kick can win the userspace exit after EL1 completed
            // its notification. ELR already points past SVC: treating this as
            // an ordinary yield would silently skip the syscall. Only a host
            // acknowledgement or the restore's complete=0 retires the request.
            return Ok((mailbox.complete == 1).then_some(mailbox));
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
        let stepping = state.nzcv & (1 << 21) != 0;
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
            // The fault was handled above. An interrupt during its private
            // restore must not publish that completed notification again.
            reply.complete = 0;
            reply.context = u64::from(self.asid) << 48;
            reply.invalidate = std::mem::take(&mut self.invalidate);
            write(&self.mailbox, 0, reply);
            if stepping {
                // A fault has not retired the original EL0 instruction. Retry
                // through the debug restore, rather than the normal HVC branch.
                self.reset_resume(true)?;
            }
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
    fn repeated_deadlines_preserve_the_entire_el0_register_bank() {
        use trames_memory::{Memory, Permissions};
        let mut memory = Memory::default();
        memory.map(0x10000, 4096, Permissions::RW).unwrap();
        let mut code = Vec::new();
        for reg in (0..31u32).filter(|&reg| reg != 9) {
            code.push(0x91000400 | (reg << 5) | reg); // ADD Xn,Xn,#1
        }
        code.push(0xf1000529); // SUBS X9,X9,#1
        code.push(0x54000001 | (((-(code.len() as i32)) as u32 & 0x7ffff) << 5));
        code.push(0xd4000001);
        for (i, instruction) in code.iter().enumerate() {
            memory
                .write_array(0x10000 + i as u64 * 4, instruction.to_le_bytes())
                .unwrap();
        }
        memory.protect(0x10000, 4096, Permissions::RX).unwrap();
        memory.prepare_hardware().unwrap();
        let mut registry = trames_memory::parallel::Registry::default();
        registry.use_hardware();
        let mut view = memory.parallel_view(&mut registry).unwrap();
        view.enable_hardware();
        let mut executor = Executor::new().unwrap();
        let mut state = Aarch64State {
            registers: std::array::from_fn(|i| 0x1000 + i as u64),
            vectors: std::array::from_fn(|i| 0x112233445566778899aabbccdd000000u128 + i as u128),
            pc: 0x10000,
            sp: 0x90000,
            tpidr_el0: 0x12345678,
            ..Default::default()
        };
        state.registers[9] = 500_000;
        let mut expected = state.clone();
        for (i, reg) in expected.registers.iter_mut().enumerate() {
            *reg = if i == 9 { 0 } else { *reg + 500_000 };
        }
        expected.pc += code.len() as u64 * 4;
        expected.nzcv = 0x60000000;
        let mut yields = 0;
        loop {
            match executor
                .run(&mut state, &mut view, Duration::from_micros(100))
                .unwrap()
                .reason
            {
                ExitReason::Syscall => break,
                ExitReason::Yield => {
                    yields += 1;
                    assert!(yields < 10_000, "bounded hardware loop must finish");
                }
            }
        }
        assert!(yields > 2, "exercise repeated asynchronous checkpoints");
        assert_eq!(state, expected);
    }
    #[test]
    #[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
    fn independent_executors_keep_user_and_private_translations_separate() {
        use trames_memory::{Memory, Permissions};
        struct Affinity(libc::cpu_set_t);
        impl Drop for Affinity {
            fn drop(&mut self) {
                assert_eq!(
                    unsafe { libc::sched_setaffinity(0, size_of::<libc::cpu_set_t>(), &self.0) },
                    0
                );
            }
        }
        let mut original: libc::cpu_set_t = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::sched_getaffinity(0, size_of::<libc::cpu_set_t>(), &mut original) },
            0
        );
        let affinity = Affinity(original);
        let cpu = (0..libc::CPU_SETSIZE as usize)
            .find(|&i| unsafe { libc::CPU_ISSET(i, &affinity.0) })
            .unwrap();
        let mut single: libc::cpu_set_t = unsafe { std::mem::zeroed() };
        unsafe { libc::CPU_SET(cpu, &mut single) };
        assert_eq!(
            unsafe { libc::sched_setaffinity(0, size_of::<libc::cpu_set_t>(), &single) },
            0
        );
        let mut memories = [Memory::default(), Memory::default()];
        let mut registry = trames_memory::parallel::Registry::default();
        registry.use_hardware();
        for (i, memory) in memories.iter_mut().enumerate() {
            memory.map(0x10000, 4096, Permissions::RW).unwrap();
            // Original MOV x19,#value; LDR x18,[x20]; SVC, with distinct code/data.
            memory
                .write_array(
                    0x10000,
                    (0xd2800013u32 | ((41 + i as u32) << 5)).to_le_bytes(),
                )
                .unwrap();
            memory
                .write_array(0x10004, 0xf9400292u32.to_le_bytes())
                .unwrap();
            memory
                .write_array(0x10008, 0xd4000001u32.to_le_bytes())
                .unwrap();
            memory.protect(0x10000, 4096, Permissions::RX).unwrap();
            memory.map(0x20000, 4096, Permissions::RW).unwrap();
            memory
                .write_array(0x20000, (i as u64 + 41).to_le_bytes())
                .unwrap();
            memory.prepare_hardware().unwrap();
        }
        let mut executors = [Executor::new().unwrap(), Executor::new().unwrap()];
        let mut views = memories
            .iter_mut()
            .map(|m| m.parallel_view(&mut registry).unwrap())
            .collect::<Vec<_>>();
        for view in &mut views {
            view.enable_hardware();
        }
        for separate in [true, false] {
            for _ in 0..32 {
                for i in 0..2 {
                    let mut state = Aarch64State {
                        pc: 0x10000,
                        ..Default::default()
                    };
                    state.registers[20] = 0x20000;
                    state.registers[8] = 172 + i as u64;
                    let executor = &mut executors[if separate { i } else { 0 }];
                    assert_eq!(
                        executor
                            .run(&mut state, &mut views[i], Duration::from_millis(100))
                            .unwrap()
                            .reason,
                        ExitReason::Syscall
                    );
                    assert_eq!(
                        (
                            state.pc,
                            state.registers[18],
                            state.registers[19],
                            state.registers[8]
                        ),
                        (0x1000c, i as u64 + 41, i as u64 + 41, 172 + i as u64)
                    );
                }
            }
        }
    }
    #[test]
    #[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
    fn interrupted_completed_notification_preserves_the_pending_syscall() {
        use trames_memory::{Memory, Permissions};
        let mut memory = Memory::default();
        memory.map(0x10000, 4096, Permissions::RW).unwrap();
        for (index, instruction) in [0xd2801588u32, 0xd4000001, 0x91000673, 0xd4000001]
            .iter()
            .enumerate()
        {
            memory
                .write_array(0x10000 + index as u64 * 4, instruction.to_le_bytes())
                .unwrap();
        }
        memory.protect(0x10000, 4096, Permissions::RX).unwrap();
        memory.prepare_hardware().unwrap();
        let mut registry = trames_memory::parallel::Registry::default();
        registry.use_hardware();
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
        // Reproduce a stopped completed EL1 notification followed by EINTR,
        // before the outer runtime has consumed its request. No guest opcode
        // or execution memory is patched to force this boundary.
        unsafe {
            (&raw mut (*executor.run).immediate_exit).write_volatile(1);
        }
        assert!(matches!(executor.event().unwrap(), Event::Interrupted));
        let request = executor
            .checkpoint(&mut state)
            .unwrap()
            .expect("completed SVC must remain pending");
        assert_eq!(
            (request.event, state.pc, state.registers[8]),
            (1, 0x10008, 172)
        );
        state.registers[0] = 99;
        assert_eq!(
            executor.run(&mut state, &mut view, quantum).unwrap().reason,
            ExitReason::Syscall
        );
        assert_eq!(
            (state.pc, state.registers[19], state.registers[0]),
            (0x10010, 1, 99)
        );
    }

    #[test]
    #[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
    fn interrupted_restore_preserves_a_pending_read_only_revocation() {
        use trames_memory::{Memory, Permissions};
        let mut memory = Memory::default();
        memory.map(0x10000, PAGE as u64, Permissions::RW).unwrap();
        // str x0, [x1]; svc #0. The first execution populates a writable TLB
        // entry; the original instructions are retained for the protected retry.
        memory
            .write_array(0x10000, 0xf9000020u32.to_le_bytes())
            .unwrap();
        memory
            .write_array(0x10004, 0xd4000001u32.to_le_bytes())
            .unwrap();
        memory
            .protect(0x10000, PAGE as u64, Permissions::RX)
            .unwrap();
        memory.map(0x20000, PAGE as u64, Permissions::RW).unwrap();
        memory.prepare_hardware().unwrap();
        let mut registry = trames_memory::parallel::Registry::default();
        registry.use_hardware();
        let mut view = memory.parallel_view(&mut registry).unwrap();
        view.enable_hardware();
        let mut executor = Executor::new().unwrap();
        let mut state = Aarch64State {
            pc: 0x10000,
            ..Default::default()
        };
        state.registers[1] = 0x20000;
        let quantum = Duration::from_millis(100);
        assert_eq!(
            executor.run(&mut state, &mut view, quantum).unwrap().reason,
            ExitReason::Syscall
        );
        drop(view);
        memory
            .protect(0x20000, PAGE as u64, Permissions::R)
            .unwrap();
        let mut view = memory.parallel_view(&mut registry).unwrap();
        view.enable_hardware();
        state.pc = 0x10000;
        executor.synchronize(&view).unwrap();
        executor.start(&state).unwrap();
        // Force a kick before the first restore instruction, when the pending
        // TLBI belongs to the mailbox rather than the host's flags.
        unsafe {
            (&raw mut (*executor.run).immediate_exit).write_volatile(1);
        }
        assert!(matches!(executor.event().unwrap(), Event::Interrupted));
        assert!(executor.checkpoint(&mut state).unwrap().is_none());
        assert_ne!(executor.invalidate & 1, 0);
        assert!(matches!(
            executor.run(&mut state, &mut view, quantum),
            Err(super::EngineError::Memory(MemoryError::Permission {
                address: 0x20000,
                ..
            }))
        ));
    }

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
    fn address_space_switch_retains_shared_code_and_withdraws_private_data() {
        use trames_memory::{Memory, Permissions};
        let mut original = Memory::default();
        original.map(0x10000, 4096, Permissions::RW).unwrap();
        original
            .write_array(0x10000, 0xd4000001u32.to_le_bytes())
            .unwrap();
        original.protect(0x10000, 4096, Permissions::RX).unwrap();
        original.map(0x20000, 4096, Permissions::RW).unwrap();
        original.write_array(0x20000, 1u64.to_le_bytes()).unwrap();
        let mut other = original.clone();
        other.write_array(0x20000, 2u64.to_le_bytes()).unwrap();
        let mut registry = trames_memory::parallel::Registry::default();
        registry.use_hardware();
        original.prepare_hardware().unwrap();
        other.prepare_hardware().unwrap();
        let mut first = original.parallel_view(&mut registry).unwrap();
        let mut second = other.parallel_view(&mut registry).unwrap();
        first.enable_hardware();
        second.enable_hardware();
        let mut executor = Executor::new().unwrap();
        let mut state = Aarch64State {
            pc: 0x10000,
            ..Default::default()
        };
        let quantum = Duration::from_millis(100);
        executor.run(&mut state, &mut first, quantum).unwrap();
        let first_context = get(&executor.cpu, system_register(3, 0, 2, 0, 0)).unwrap() >> 48;
        let shared = executor.pages[&0x10000].info.storage_id;
        // Force one data mapping exactly as the real data-abort admission path.
        executor
            .fault(
                &first,
                &Mailbox {
                    syndrome: (0x24 << 26) | 4,
                    address: 0x20000,
                    ..Default::default()
                },
            )
            .unwrap();
        let private = executor.pages[&0x20000].info.storage_id;
        executor.synchronize(&second).unwrap();
        let second_context = get(&executor.cpu, system_register(3, 0, 2, 0, 0)).unwrap() >> 48;
        assert_ne!(
            first_context, second_context,
            "cached roots require distinct translation contexts"
        );
        assert!(executor.pages.is_empty());
        state.pc = 0x10000;
        executor.run(&mut state, &mut second, quantum).unwrap();
        executor
            .fault(
                &second,
                &Mailbox {
                    syndrome: (0x24 << 26) | 4,
                    address: 0x20000,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_ne!(executor.pages[&0x20000].info.storage_id, private);
        let runs = executor.run_calls;
        for _ in 0..8 {
            executor.synchronize(&first).unwrap();
            assert_eq!(
                get(&executor.cpu, system_register(3, 0, 2, 0, 0)).unwrap() >> 48,
                first_context
            );
            assert_eq!(executor.pages[&0x10000].info.storage_id, shared);
            assert_eq!(executor.pages[&0x20000].info.storage_id, private);
            state.pc = 0x10000;
            executor.run(&mut state, &mut first, quantum).unwrap();
            state.pc = 0x10000;
            executor.run(&mut state, &mut second, quantum).unwrap();
        }
        assert_eq!(
            executor.run_calls - runs,
            if executor.vm.smccc { 16 } else { 32 }
        );
        drop(first);
        original.unmap(0x20000, 4096).unwrap();
        let mut changed = original.parallel_view(&mut registry).unwrap();
        changed.enable_hardware();
        executor.synchronize(&changed).unwrap();
        assert!(!executor.pages.contains_key(&0x20000));
        assert_eq!(executor.pages[&0x10000].info.storage_id, shared);
    }
    #[test]
    #[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
    fn address_space_cache_is_bounded_and_does_not_retain_retired_guest_pages() {
        use trames_memory::{Memory, Permissions};
        let mut executor = Executor::new().unwrap();
        for _ in 0..CACHED_SPACES + 4 {
            let mut memory = Memory::default();
            memory.map(0x10000, 4096, Permissions::RW).unwrap();
            memory
                .write_array(0x10000, 0xd4000001u32.to_le_bytes())
                .unwrap();
            memory.protect(0x10000, 4096, Permissions::RX).unwrap();
            memory.prepare_hardware().unwrap();
            let mut registry = trames_memory::parallel::Registry::default();
            registry.use_hardware();
            let mut view = memory.parallel_view(&mut registry).unwrap();
            view.enable_hardware();
            executor
                .run(
                    &mut Aarch64State {
                        pc: 0x10000,
                        ..Default::default()
                    },
                    &mut view,
                    Duration::from_millis(100),
                )
                .unwrap();
            let owner = executor.pages[&0x10000].owner.clone();
            let contexts = std::iter::once(executor.asid)
                .chain(executor.spaces.values().map(|space| space.asid))
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(contexts.len(), executor.spaces.len() + 1);
            assert!(contexts
                .iter()
                .all(|&asid| u32::from(asid) >= u32::from(executor.asid_base)
                    && u32::from(asid)
                        < u32::from(executor.asid_base) + u32::from(executor.asid_count)));
            assert!(owner.upgrade().is_some());
            assert!(executor.spaces.len() < CACHED_SPACES);
            drop(view);
            drop(memory);
            drop(registry);
            assert!(
                owner.upgrade().is_none(),
                "inactive roots cannot pin guest RAM"
            );
        }
        assert_eq!(
            executor.spaces.len(),
            CACHED_SPACES.min(executor.asid_count as usize) - 1
        );
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
