// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical memfd RAM. Only the KVM alias is revoked during host byte borrows.
use crate::{Error, Result};
use kvm_bindings::*;
use kvm_ioctls::{Kvm, VmFd};
use std::{
    collections::{BTreeMap, BTreeSet},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    ptr::{self, NonNull},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
};
use trames_memory::{HardwareRange, HardwareStorage, HardwareWait, MemoryError};

pub(crate) const PAGE: usize = 4096;
const ARENA: usize = 64 << 20;
const GPA_BASE: u64 = 0x4000_0000;
mod imports;
use imports::ImportedChunk;
static CURRENT: Mutex<Weak<Vm>> = Mutex::new(Weak::new());

pub(crate) struct Vm {
    pub fd: VmFd,
    pub kvm: Kvm,
    pub smccc: bool,
    arenas: Mutex<Vec<Weak<Arena>>>,
    imports: Mutex<BTreeMap<(u64, u64, u64), Weak<ImportedChunk>>>,
    slots: Mutex<BTreeSet<u32>>,
    cpus: Mutex<BTreeSet<u64>>,
    leases: Mutex<Leases>,
}
#[derive(Default)]
struct Leases {
    count: usize,
    generation: u64,
    waiters: Vec<Arc<HardwareWait>>,
}
impl Vm {
    pub fn shared() -> Result<Arc<Self>> {
        let mut current = CURRENT.lock().unwrap();
        if let Some(vm) = current.upgrade() {
            return Ok(vm);
        }
        crate::Doctor::inspect().require_probe()?;
        let kvm = Kvm::new().map_err(|e| Error::context("open shared KVM", e))?;
        let fd = kvm
            .create_vm()
            .map_err(|e| Error::context("create shared KVM", e))?;
        let requested: crate::Transport = std::env::var("TRAMES_KVM_TRANSPORT")
            .unwrap_or_else(|_| "auto".into())
            .parse()?;
        let (smccc, ipa_limit) = crate::aarch64::engine::configure_vm(&fd, requested)?;
        let maximum = kvm
            .get_nr_memslots()
            .min((ipa_limit - GPA_BASE) as usize / ARENA);
        let vm = Arc::new(Self {
            fd,
            kvm,
            smccc,
            arenas: Mutex::new(Vec::new()),
            imports: Mutex::new(BTreeMap::new()),
            slots: Mutex::new((0..maximum as u32).collect()),
            cpus: Mutex::new(BTreeSet::new()),
            leases: Mutex::new(Leases::default()),
        });
        *current = Arc::downgrade(&vm);
        Ok(vm)
    }
    pub fn cpu_id(&self) -> Result<u64> {
        let mut ids = self.cpus.lock().unwrap();
        let id = (0..self.kvm.get_max_vcpus() as u64)
            .find(|id| !ids.contains(id))
            .ok_or_else(|| Error("KVM vCPU capacity exhausted".into()))?;
        ids.insert(id);
        Ok(id)
    }
    pub fn release_cpu(&self, id: u64) {
        self.cpus.lock().unwrap().remove(&id);
    }
    pub fn allocate(self: &Arc<Self>, length: usize) -> Result<Arc<Allocation>> {
        assert!(length > 0 && length.is_multiple_of(PAGE) && length <= ARENA);
        let mut arenas = self.arenas.lock().unwrap();
        arenas.retain(|a| a.strong_count() != 0);
        for arena in arenas.iter().filter_map(Weak::upgrade) {
            if let Some(offset) = arena.claim(length) {
                return Ok(Allocation::new(arena, offset, length));
            }
        }
        let slot = self
            .slots
            .lock()
            .unwrap()
            .pop_first()
            .ok_or_else(|| Error("bounded KVM RAM slots exhausted".into()))?;
        let arena = match Arena::new(self, slot) {
            Ok(a) => Arc::new(a),
            Err(e) => {
                self.slots.lock().unwrap().insert(slot);
                return Err(e);
            }
        };
        let offset = arena.claim(length).unwrap();
        arenas.push(Arc::downgrade(&arena));
        Ok(Allocation::new(arena, offset, length))
    }
    pub fn page(
        self: &Arc<Self>,
        bytes: Option<&[u8]>,
    ) -> std::result::Result<Arc<dyn HardwareStorage>, MemoryError> {
        let allocation = self
            .allocate(PAGE)
            .map_err(|_| MemoryError::BackingIo(libc::ENOMEM as u32))?;
        if let Some(bytes) = bytes {
            assert_eq!(bytes.len(), PAGE);
            // SAFETY: fresh allocation has no published hardware or host actor.
            unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), allocation.pointer(), PAGE);
            }
        }
        Ok(allocation)
    }
    pub fn range(self: &Arc<Self>) -> std::result::Result<Arc<dyn HardwareRange>, MemoryError> {
        self.allocate(trames_memory::HARDWARE_MAPPING_CHUNK_SIZE as usize)
            .map(|a| a as Arc<dyn HardwareRange>)
            .map_err(|_| MemoryError::BackingIo(libc::ENOMEM as u32))
    }
    pub fn wait_for_lease(&self) -> (Option<Arc<HardwareWait>>, u64) {
        let mut leases = self.leases.lock().unwrap();
        let wait = if leases.count == 0 {
            None
        } else {
            let wait = Arc::new(HardwareWait::default());
            leases.waiters.push(wait.clone());
            Some(wait)
        };
        (wait, leases.generation)
    }
    pub fn slot_count(&self) -> usize {
        let private = self
            .arenas
            .lock()
            .unwrap()
            .iter()
            .filter(|a| a.strong_count() != 0)
            .count();
        private
            + self
                .imports
                .lock()
                .unwrap()
                .values()
                .filter(|chunk| chunk.strong_count() != 0)
                .count()
    }
    pub fn publish_code(&self, gpa: u64, pointer: *mut u8) -> Result<()> {
        let arenas = self.arenas.lock().unwrap();
        if let Some(arena) = arenas
            .iter()
            .filter_map(Weak::upgrade)
            .find(|a| (a.gpa..a.gpa + ARENA as u64).contains(&gpa))
        {
            let offset = (gpa - arena.gpa) as usize;
            arena.executable[offset / PAGE].store(true, Ordering::Relaxed);
        } else {
            drop(arenas);
            self.publish_imported_code(gpa)?;
        }
        publish(pointer, PAGE);
        Ok(())
    }
}
struct Mapping {
    pointer: NonNull<u8>,
    length: usize,
}
impl Mapping {
    fn new(fd: &OwnedFd) -> Result<Self> {
        // SAFETY: independent MAP_SHARED views of the same retained memfd.
        let p = unsafe {
            libc::mmap(
                ptr::null_mut(),
                ARENA,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        if p == libc::MAP_FAILED {
            return Err(Error::context(
                "map canonical RAM",
                std::io::Error::last_os_error(),
            ));
        }
        Ok(Self {
            pointer: NonNull::new(p.cast()).unwrap(),
            length: ARENA,
        })
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.pointer.as_ptr().cast(), self.length);
        }
    }
}
struct Pool {
    used: Vec<bool>,
    cursor: usize,
}
struct Arena {
    host: Mapping,
    hardware: Mapping,
    _file: OwnedFd,
    vm: Weak<Vm>,
    slot: u32,
    gpa: u64,
    allocated: Mutex<Pool>,
    executable: Vec<AtomicBool>,
}
// SAFETY: mappings are stable; no references are exposed without the allocation
// gate. The kernel invalidates every hardware alias before mprotect returns.
unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}
impl Arena {
    fn new(vm: &Arc<Vm>, slot: u32) -> Result<Self> {
        let raw = unsafe {
            libc::memfd_create(
                c"trames-ram".as_ptr(),
                libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
            )
        };
        if raw < 0 {
            return Err(Error::context("memfd RAM", std::io::Error::last_os_error()));
        }
        let file = unsafe { OwnedFd::from_raw_fd(raw) };
        if unsafe { libc::ftruncate(file.as_raw_fd(), ARENA as _) } != 0 {
            return Err(Error::context("size RAM", std::io::Error::last_os_error()));
        }
        if unsafe {
            libc::fcntl(
                file.as_raw_fd(),
                libc::F_ADD_SEALS,
                libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_SEAL,
            )
        } < 0
        {
            return Err(Error::context(
                "seal RAM geometry",
                std::io::Error::last_os_error(),
            ));
        }
        let host = Mapping::new(&file)?;
        let hardware = Mapping::new(&file)?;
        let gpa = GPA_BASE + slot as u64 * ARENA as u64;
        let region = kvm_userspace_memory_region {
            slot,
            flags: 0,
            guest_phys_addr: gpa,
            memory_size: ARENA as u64,
            userspace_addr: hardware.pointer.as_ptr() as u64,
        };
        // SAFETY: Arena and each installed PTE retain these complete mappings.
        unsafe { vm.fd.set_user_memory_region(region) }
            .map_err(|e| Error::context("register RAM arena", e))?;
        Ok(Self {
            host,
            hardware,
            _file: file,
            vm: Arc::downgrade(vm),
            slot,
            gpa,
            allocated: Mutex::new(Pool {
                used: vec![false; ARENA / PAGE],
                cursor: 0,
            }),
            executable: (0..ARENA / PAGE).map(|_| AtomicBool::new(false)).collect(),
        })
    }
    fn claim(&self, length: usize) -> Option<usize> {
        let pages = length / PAGE;
        let mut pool = self.allocated.lock().unwrap();
        let index = if pages == 1 {
            (pool.cursor..pool.used.len()).find(|&i| !pool.used[i])?
        } else {
            (0..pool.used.len()).step_by(pages).find(|&i| {
                i + pages <= pool.used.len() && pool.used[i..i + pages].iter().all(|&used| !used)
            })?
        };
        pool.used[index..index + pages].fill(true);
        if index == pool.cursor {
            pool.cursor = index + pages;
        }
        Some(index * PAGE)
    }
}
impl Drop for Arena {
    fn drop(&mut self) {
        if let Some(vm) = self.vm.upgrade() {
            let region = kvm_userspace_memory_region {
                slot: self.slot,
                ..Default::default()
            };
            if unsafe { vm.fd.set_user_memory_region(region) }.is_err() {
                // Failing to withdraw must never free still-registered bytes.
                // This is a host transport invariant, so fail closed.
                std::process::abort();
            }
            vm.slots.lock().unwrap().insert(self.slot);
        }
    }
}
pub(crate) struct Allocation {
    arena: Arc<Arena>,
    offset: usize,
    length: usize,
    hosts: Mutex<usize>,
}
impl Allocation {
    fn new(arena: Arc<Arena>, offset: usize, length: usize) -> Arc<Self> {
        // Last owner retirement punched a hole; new memfd pages are demand-zero.
        Arc::new(Self {
            arena,
            offset,
            length,
            hosts: Mutex::new(0),
        })
    }
    pub fn gpa(&self) -> u64 {
        self.arena.gpa + self.offset as u64
    }
    fn hardware(&self) -> *mut u8 {
        unsafe { self.arena.hardware.pointer.as_ptr().add(self.offset) }
    }
    pub fn publish(&self) {
        publish(self.pointer(), self.length);
    }
}
// SAFETY: same retained memfd bytes in host and hardware views. A first host
// borrower revokes only the hardware view synchronously through MMU notifiers.
// Original SharedPage region locks arbitrate host/host byte overlaps.
unsafe impl HardwareStorage for Allocation {
    fn pointer(&self) -> *mut u8 {
        unsafe { self.arena.host.pointer.as_ptr().add(self.offset) }
    }
    fn physical_address(&self) -> u64 {
        self.gpa()
    }
    fn is_live(&self) -> bool {
        self.arena.vm.strong_count() != 0
    }
    fn page_resident(&self) -> std::result::Result<u8, MemoryError> {
        let mut pages = [0];
        self.residency(0, &mut pages)?;
        Ok(pages[0])
    }
    fn publish_host_write(&self, offset: usize, length: usize) {
        assert!(offset <= self.length && length <= self.length - offset);
        if length != 0 {
            let first = (self.offset + offset) / PAGE;
            let last = (self.offset + offset + length - 1) / PAGE;
            if (first..=last).any(|i| self.arena.executable[i].load(Ordering::Relaxed)) {
                publish(unsafe { self.pointer().add(offset) }, length);
            }
        }
    }
    fn begin_host_access(&self) {
        begin_alias(&self.arena.vm, &self.hosts, self.hardware(), self.length);
    }
    fn end_host_access(&self) {
        end_alias(&self.arena.vm, &self.hosts, self.hardware(), self.length);
    }
}
// SAFETY: complete stable initialized range; residency is kernel metadata only.
unsafe impl HardwareRange for Allocation {
    fn len(&self) -> usize {
        self.length
    }
    fn resident_bytes(&self) -> usize {
        let mut pages = vec![0; self.length / PAGE];
        self.residency(0, &mut pages).map_or(self.length, |_| {
            pages.iter().filter(|&&p| p != 0).count() * PAGE
        })
    }
    fn residency(&self, offset: usize, pages: &mut [u8]) -> std::result::Result<(), MemoryError> {
        if !offset.is_multiple_of(PAGE)
            || offset > self.length
            || pages.len() > (self.length - offset) / PAGE
        {
            return Err(MemoryError::InvalidRange);
        }
        if pages.is_empty() {
            return Ok(());
        }
        if unsafe {
            libc::mincore(
                self.pointer().add(offset).cast(),
                pages.len() * PAGE,
                pages.as_mut_ptr(),
            )
        } != 0
        {
            return Err(MemoryError::BackingIo(libc::EIO as u32));
        }
        for page in pages {
            *page &= 1;
        }
        Ok(())
    }
    // Keep the default populated_pages: swapped/nonresident bytes may be dirty.
}
impl Drop for Allocation {
    fn drop(&mut self) {
        assert_eq!(*self.hosts.get_mut().unwrap(), 0);
        // No PTE/borrow owner remains. Punching the shared memfd frees physical
        // pages and zeros both aliases before this bounded extent can be reused.
        if unsafe {
            libc::fallocate(
                self.arena._file.as_raw_fd(),
                libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE,
                self.offset as _,
                self.length as _,
            )
        } != 0
        {
            std::process::abort();
        }
        let mut pool = self.arena.allocated.lock().unwrap();
        pool.used[self.offset / PAGE..(self.offset + self.length) / PAGE].fill(false);
        pool.cursor = pool.cursor.min(self.offset / PAGE);
    }
}
pub(crate) fn publish(pointer: *mut u8, length: usize) {
    if length == 0 {
        return;
    }
    unsafe {
        let ctr: u64;
        core::arch::asm!("mrs {0}, ctr_el0", out(reg) ctr, options(nomem, nostack));
        let dline = 4usize << ((ctr >> 16) & 15);
        let iline = 4usize << (ctr & 15);
        let begin = pointer as usize;
        let end = begin + length;
        for a in (begin / dline * dline..end).step_by(dline) {
            core::arch::asm!("dc cvau, {0}",in(reg)a,options(nostack));
        }
        core::arch::asm!("dsb ish", options(nostack));
        for a in (begin / iline * iline..end).step_by(iline) {
            core::arch::asm!("ic ivau, {0}",in(reg)a,options(nostack));
        }
        core::arch::asm!("dsb ish", "isb", options(nostack));
    }
}

fn begin_alias(weak: &Weak<Vm>, hosts: &Mutex<usize>, pointer: *mut u8, length: usize) {
    let mut hosts = hosts.lock().unwrap();
    if *hosts == 0 {
        if let Some(vm) = weak.upgrade() {
            {
                let mut leases = vm.leases.lock().unwrap();
                leases.count += 1;
                leases.generation = leases.generation.wrapping_add(1);
            }
            assert_eq!(
                unsafe { libc::mprotect(pointer.cast(), length, libc::PROT_NONE) },
                0,
                "cannot revoke KVM byte access"
            );
        }
    }
    *hosts += 1;
}
fn end_alias(weak: &Weak<Vm>, hosts: &Mutex<usize>, pointer: *mut u8, length: usize) {
    let mut hosts = hosts.lock().unwrap();
    assert!(*hosts != 0);
    *hosts -= 1;
    if *hosts == 0 {
        if let Some(vm) = weak.upgrade() {
            assert_eq!(
                unsafe {
                    libc::mprotect(pointer.cast(), length, libc::PROT_READ | libc::PROT_WRITE)
                },
                0,
                "cannot restore KVM byte access"
            );
            let waiters = {
                let mut leases = vm.leases.lock().unwrap();
                leases.count -= 1;
                leases.generation = leases.generation.wrapping_add(1);
                std::mem::take(&mut leases.waiters)
            };
            drop(hosts);
            for wait in waiters {
                wait.fire();
            }
        }
    }
}
