// SPDX-License-Identifier: GPL-3.0-or-later
//! Revocable aliases of explicit sealed-file exports; never guess a GPU pointer.
use super::*;
use trames_memory::SharedMapping;
struct PageGate {
    hosts: Mutex<usize>,
}
pub(super) struct ImportedChunk {
    hardware: Mapping,
    _file: OwnedFd,
    vm: Weak<Vm>,
    slot: u32,
    gpa: u64,
    start: u64,
    length: usize,
    gates: Mutex<BTreeMap<usize, Weak<PageGate>>>,
    executable: Vec<AtomicBool>,
}
// SAFETY: mmap remains fixed; original file geometry is sealed, and shared gates
// revoke all KVM aliases of a file page before original Rust byte guards return.
unsafe impl Send for ImportedChunk {}
unsafe impl Sync for ImportedChunk {}
impl ImportedChunk {
    fn pointer(&self, index: usize) -> *mut u8 {
        unsafe { self.hardware.pointer.as_ptr().add(index * PAGE) }
    }
    fn gate(&self, index: usize) -> Arc<PageGate> {
        let mut gates = self.gates.lock().unwrap();
        if let Some(gate) = gates.get(&index).and_then(Weak::upgrade) {
            return gate;
        }
        let gate = Arc::new(PageGate {
            hosts: Mutex::new(0),
        });
        gates.insert(index, Arc::downgrade(&gate));
        gate
    }
}
impl Drop for ImportedChunk {
    fn drop(&mut self) {
        if let Some(vm) = self.vm.upgrade() {
            let region = kvm_userspace_memory_region {
                slot: self.slot,
                ..Default::default()
            };
            if unsafe { vm.fd.set_user_memory_region(region) }.is_err() {
                std::process::abort();
            }
            vm.slots.lock().unwrap().insert(self.slot);
        }
    }
}
struct Page {
    chunk: Arc<ImportedChunk>,
    index: usize,
    gate: Arc<PageGate>,
    mapping: SharedMapping,
}
// SAFETY: pointer is the exact original page, retained by the provider mapping.
// All duplicate file-page imports share the same revocable hardware alias/gate.
// Original SharedPage guards retain host/host alias and device API responsibility.
unsafe impl HardwareStorage for Page {
    fn pointer(&self) -> *mut u8 {
        self.mapping.pointer()
    }
    fn physical_address(&self) -> u64 {
        self.chunk.gpa + self.index as u64 * PAGE as u64
    }
    fn is_live(&self) -> bool {
        self.chunk.vm.strong_count() != 0
    }
    fn shared_mapping(&self) -> Option<SharedMapping> {
        Some(self.mapping.clone())
    }
    fn begin_host_access(&self) {
        begin_alias(
            &self.chunk.vm,
            &self.gate.hosts,
            self.chunk.pointer(self.index),
            PAGE,
        );
    }
    fn end_host_access(&self) {
        end_alias(
            &self.chunk.vm,
            &self.gate.hosts,
            self.chunk.pointer(self.index),
            PAGE,
        );
    }
    fn publish_host_write(&self, offset: usize, length: usize) {
        assert!(offset <= PAGE && length <= PAGE - offset);
        if length != 0 && self.chunk.executable[self.index].load(Ordering::Relaxed) {
            publish(unsafe { self.pointer().add(offset) }, length);
        }
    }
    fn page_resident(&self) -> std::result::Result<u8, MemoryError> {
        let mut page = 0u8;
        if unsafe { libc::mincore(self.pointer().cast(), PAGE, &mut page) } != 0 {
            return Err(MemoryError::BackingIo(libc::EIO as u32));
        }
        Ok(page & 1)
    }
}
impl Vm {
    pub fn import(
        self: &Arc<Self>,
        mapping: SharedMapping,
    ) -> std::result::Result<Arc<dyn HardwareStorage>, MemoryError> {
        let unsupported = || MemoryError::BackingIo(libc::EOPNOTSUPP as u32);
        let fd = mapping.file_descriptor().ok_or_else(unsupported)?;
        use std::os::fd::AsRawFd;
        let seals = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GET_SEALS) };
        if seals < 0
            || seals & (libc::F_SEAL_GROW | libc::F_SEAL_SHRINK)
                != (libc::F_SEAL_GROW | libc::F_SEAL_SHRINK)
        {
            return Err(unsupported());
        }
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(fd.as_raw_fd(), &mut stat) } != 0
            || stat.st_size < 0
            || stat.st_mode & libc::S_IFMT != libc::S_IFREG
        {
            return Err(unsupported());
        }
        let base = mapping.file_offset().ok_or_else(unsupported)?;
        let offset = base
            .checked_add(mapping.allocation_offset() as u64)
            .ok_or_else(unsupported)?;
        let end = base
            .checked_add(mapping.allocation_len() as u64)
            .ok_or_else(unsupported)?;
        if offset % PAGE as u64 != 0
            || !(mapping.pointer() as usize).is_multiple_of(PAGE)
            || offset.checked_add(PAGE as u64).is_none_or(|n| n > end)
            || end > stat.st_size as u64
        {
            return Err(unsupported());
        }
        let identity = (stat.st_dev as u64, stat.st_ino as u64);
        let mut imports = self.imports.lock().unwrap();
        imports.retain(|_, v| v.strong_count() != 0);
        let chunks: Vec<_> = imports
            .range((identity.0, identity.1, 0)..=(identity.0, identity.1, u64::MAX))
            .filter_map(|(_, v)| v.upgrade())
            .collect();
        let chunk = if let Some(chunk) = chunks
            .iter()
            .find(|c| offset >= c.start && offset + PAGE as u64 <= c.start + c.length as u64)
        {
            chunk.clone()
        } else {
            // Disjoint file windows deduplicate even when providers export
            // overlapping suballocations or independent canonical host aliases.
            let mut start = base.max(offset / ARENA as u64 * ARENA as u64);
            let mut stop = end.min(start + ARENA as u64);
            for old in &chunks {
                if old.start + old.length as u64 <= offset {
                    start = start.max(old.start + old.length as u64);
                }
                if old.start > offset {
                    stop = stop.min(old.start);
                }
            }
            let length = usize::try_from(stop.checked_sub(start).ok_or_else(unsupported)?)
                .map_err(|_| unsupported())?;
            if length == 0 || length > ARENA || length % PAGE != 0 {
                return Err(unsupported());
            }
            let file = fd.try_clone_to_owned().map_err(|_| unsupported())?;
            let raw = unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    length,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    file.as_raw_fd(),
                    start as _,
                )
            };
            if raw == libc::MAP_FAILED {
                return Err(MemoryError::BackingIo(libc::ENOMEM as u32));
            }
            let hardware = Mapping {
                pointer: NonNull::new(raw.cast()).unwrap(),
                length,
            };
            let slot = self
                .slots
                .lock()
                .unwrap()
                .pop_first()
                .ok_or_else(unsupported)?;
            let gpa = GPA_BASE + slot as u64 * ARENA as u64;
            let region = kvm_userspace_memory_region {
                slot,
                flags: 0,
                guest_phys_addr: gpa,
                memory_size: length as u64,
                userspace_addr: raw as u64,
            };
            if unsafe { self.fd.set_user_memory_region(region) }.is_err() {
                self.slots.lock().unwrap().insert(slot);
                return Err(unsupported());
            }
            let chunk = Arc::new(ImportedChunk {
                hardware,
                _file: file,
                vm: Arc::downgrade(self),
                slot,
                gpa,
                start,
                length,
                gates: Mutex::new(BTreeMap::new()),
                executable: (0..length / PAGE).map(|_| AtomicBool::new(false)).collect(),
            });
            imports.insert((identity.0, identity.1, start), Arc::downgrade(&chunk));
            chunk
        };
        let index = ((offset - chunk.start) / PAGE as u64) as usize;
        let gate = chunk.gate(index);
        Ok(Arc::new(Page {
            chunk,
            index,
            gate,
            mapping,
        }))
    }
    pub(super) fn publish_imported_code(&self, gpa: u64) -> Result<()> {
        let imports = self.imports.lock().unwrap();
        let chunk = imports
            .values()
            .filter_map(Weak::upgrade)
            .find(|c| (c.gpa..c.gpa + c.length as u64).contains(&gpa))
            .ok_or_else(|| Error("unowned imported executable page".into()))?;
        chunk.executable[((gpa - chunk.gpa) / PAGE as u64) as usize].store(true, Ordering::Relaxed);
        Ok(())
    }
}
