// SPDX-License-Identifier: GPL-3.0-or-later
#![cfg(all(target_os = "linux", target_arch = "aarch64"))]
use trames_engine::{Cpu as _, SliceOutcome};
use trames_execution::Registers;
use trames_kvm::kernel::{Cpu, Engine};
use trames_memory::{parallel::Registry, Memory, Permissions};
const CODE: u64 = 0x7fff_0000_1000;
const DATA: u64 = 0x20000;
fn setup(words: &[u32]) -> (Cpu, Memory) {
    let mut m = Memory::default();
    m.map(CODE, 4096, Permissions::RW).unwrap();
    for (i, w) in words.iter().enumerate() {
        m.write_array(CODE + i as u64 * 4, w.to_le_bytes()).unwrap();
    }
    m.protect(CODE, 4096, Permissions::RX).unwrap();
    m.map(DATA, 4096, Permissions::RW).unwrap();
    let mut cpu = Cpu::new(CODE, 0x800000);
    cpu.set_x(10, DATA);
    cpu.set_x(8, 172);
    (cpu, m)
}
fn syscall(cpu: &mut Cpu, engine: &mut Engine, memory: &mut Memory) {
    let start = std::time::Instant::now();
    loop {
        match cpu.run(engine, memory, u64::MAX) {
            SliceOutcome::Syscall { .. } => return,
            SliceOutcome::Yield { .. } if start.elapsed() < std::time::Duration::from_secs(3) => {}
            other => panic!("expected syscall, got {other:?}, pc={:#x}", cpu.pc()),
        }
    }
}
fn word(m: &Memory) -> u64 {
    u64::from_le_bytes(m.read_array(DATA).unwrap())
}
#[test]
#[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
fn canonical_pages_fork_cow_protection_and_unmap_reuse() {
    // Original load/add/store/SVC/branch instructions, including a high guest VA.
    let (mut cpu, mut memory) =
        setup(&[0xf9400141, 0x91000421, 0xf9000141, 0xd4000001, 0x17fffffc]);
    let mut engine = Engine::default();
    syscall(&mut cpu, &mut engine, &mut memory);
    assert_eq!(word(&memory), 1);
    memory.map(DATA + 4096, 4096, Permissions::R).unwrap();
    let mut child = memory.try_fork().unwrap();
    let mut child_cpu = cpu.clone();
    memory.write_array(DATA, 100u64.to_le_bytes()).unwrap();
    syscall(&mut cpu, &mut engine, &mut memory);
    assert_eq!(word(&memory), 101);
    let mut other = Engine::default();
    syscall(&mut child_cpu, &mut other, &mut child);
    assert_eq!(word(&child), 2);
    assert_eq!(word(&memory), 101);
    memory.protect(DATA, 4096, Permissions::R).unwrap();
    loop {
        match cpu.run(&mut engine, &mut memory, u64::MAX) {
            SliceOutcome::Yield { .. } => {}
            SliceOutcome::Fault(trames_engine::CpuFault::Memory {
                error: trames_memory::MemoryError::Permission { .. },
                ..
            }) => break,
            other => panic!("write protection: {other:?}"),
        }
    }
    memory.unmap(DATA, 4096).unwrap();
    memory.map(DATA, 4096, Permissions::RW).unwrap();
    cpu.set_pc(CODE);
    syscall(&mut cpu, &mut engine, &mut memory);
    assert_eq!(word(&memory), 1);
    assert!(
        engine.ram_slots() <= 2,
        "RAM registrations must be arenas rather than one slot per page"
    );
    for address in [1u64 << 48, 0xffff_ffff_ffff_a000] {
        cpu.set_pc(CODE);
        cpu.set_x(10, address);
        let begin = std::time::Instant::now();
        loop {
            match cpu.run(&mut engine, &mut memory, u64::MAX) {
                SliceOutcome::Yield { .. }
                    if begin.elapsed() < std::time::Duration::from_secs(3) => {}
                SliceOutcome::Fault(trames_engine::CpuFault::Memory {
                    error:
                        trames_memory::MemoryError::Unmapped(at)
                        | trames_memory::MemoryError::AddressRange(at),
                    ..
                }) => {
                    // Shared CpuMemory strips the top byte for both software
                    // and hardware actors; compare that same address domain.
                    assert_eq!(at & 0x00ff_ffff_ffff_ffff, address & 0x00ff_ffff_ffff_ffff);
                    break;
                }
                other => {
                    panic!("invalid/private address must remain a guest mapping fault: {other:?}")
                }
            }
        }
    }
}
#[test]
#[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
fn deadline_state_and_preentry_kick_survive_worker_migration() {
    let (mut cpu, mut memory) = setup(&[0x91000442, 0x17ffffff]); // add x2,x2,#1; b -4
    cpu.state.vectors = std::array::from_fn(|i| 0x112233445566778899aabbccdd000000u128 + i as u128);
    cpu.set_tls(0x12345678);
    cpu.state.nzcv = 0xa0000000;
    cpu.state.fpcr = 0x03400000;
    cpu.state.fpsr = 0x0800009f;
    let saved = cpu.state.clone();
    let mut first = Engine::default();
    let kick = Cpu::execution_interrupt(&mut first).unwrap().unwrap();
    kick.request_exit();
    assert!(matches!(
        cpu.run(&mut first, &mut memory, u64::MAX),
        SliceOutcome::Yield { .. }
    ));
    assert_eq!(cpu.state, saved);
    let mut second = Engine::default();
    let mut previous = 0;
    for i in 0..60 {
        let engine = if i % 2 == 0 { &mut first } else { &mut second };
        assert!(matches!(
            cpu.run(engine, &mut memory, u64::MAX),
            SliceOutcome::Yield { .. }
        ));
        assert!(cpu.x(2) >= previous);
        previous = cpu.x(2);
        assert_eq!(cpu.state.vectors, saved.vectors);
        assert_eq!(cpu.state.tpidr_el0, saved.tpidr_el0);
        assert_eq!(cpu.state.nzcv, saved.nzcv);
        assert_eq!(cpu.state.fpcr, saved.fpcr);
        assert_eq!(cpu.state.fpsr, saved.fpsr);
        assert!([CODE, CODE + 4].contains(&cpu.pc()));
    }
    assert!(previous > 0);
    drop(first);
    kick.request_exit(); // retained handle is inert after retirement
}
#[test]
#[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
fn architectural_step_retries_faults_and_survives_task_and_worker_switches() {
    let (mut cpu, mut memory) = setup(&[
        0x91000442, // ADD X2,X2,#1
        0xf9400143, // LDR X3,[X10] (first access needs a stage-one fault)
        0xd4000001, // original SVC
        0x91000442, // ADD X2,X2,#1
        0xd4000001,
    ]);
    memory
        .write(DATA, &0x123456789abcdef0u64.to_le_bytes())
        .unwrap();
    cpu.set_x(10, DATA);
    cpu.state.fpcr = 0x03400000;
    cpu.state.fpsr = 0x0800009f;
    let mut first = Engine::default();
    let mut second = Engine::default();
    for (index, engine) in [&mut first, &mut second].into_iter().enumerate() {
        cpu.set_single_step(true);
        assert!(matches!(
            cpu.run(engine, &mut memory, u64::MAX),
            SliceOutcome::Fault(trames_engine::CpuFault::Trap {
                signal: 5,
                code: 2,
                ..
            })
        ));
        assert_eq!(cpu.pc(), CODE + (index as u64 + 1) * 4);
        assert_eq!(cpu.x(2), 1);
        assert_eq!(cpu.architectural_state().nzcv & (1 << 21), 0);
        assert_eq!(cpu.state.fpcr, 0x03400000);
        assert_eq!(cpu.state.fpsr, 0x0800009f);
    }
    assert_eq!(cpu.x(3), 0x123456789abcdef0);
    // An SVC remains one original syscall, not an EL1 debug stop.
    cpu.set_single_step(true);
    assert!(matches!(
        cpu.run(&mut first, &mut memory, u64::MAX),
        SliceOutcome::Syscall { .. }
    ));
    assert_eq!(cpu.pc(), CODE + 12);
    cpu.set_single_step(false);
    syscall(&mut cpu, &mut second, &mut memory);
    assert_eq!(cpu.pc(), CODE + 20);
    assert_eq!(cpu.x(2), 2);
    // The same worker's next ordinary task must not inherit debug admission.
    let (mut ordinary, mut other_memory) = setup(&[0x91000442, 0xd4000001]);
    syscall(&mut ordinary, &mut first, &mut other_memory);
    assert_eq!(ordinary.x(2), 1);
}

#[test]
#[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
fn byte_borrow_revokes_cached_stage_two_and_wakes_without_polling() {
    let (mut cpu, mut memory) =
        setup(&[0xf9400141, 0x91000421, 0xf9000141, 0xd4000001, 0x17fffffc]);
    let mut engine = Engine::default();
    syscall(&mut cpu, &mut engine, &mut memory);
    let mut registry = Registry::default();
    Cpu::prepare_hardware_memory(&mut memory, &mut registry).unwrap();
    let mut view = memory.parallel_view(&mut registry).unwrap();
    Cpu::prepare_hardware_view(&mut view);
    let storage = view.retained_hardware_page(DATA).unwrap().0;
    storage.begin_host_access();
    let worker = std::thread::spawn(move || {
        let mut worker = Engine::default();
        let result = cpu.run_parallel(&mut worker, &mut view, u64::MAX);
        assert!(matches!(result, SliceOutcome::Yield { .. }), "{result:?}");
        let wait = Cpu::take_hardware_wait(&mut worker)
            .expect("lease must return its execution slot with a wait");
        (cpu, view, wait)
    });
    let (mut cpu, mut view, wait) = worker.join().unwrap();
    assert!(!wait.ready());
    // SAFETY: explicit gate completed KVM MMU invalidation; this sole host
    // borrower retains storage, and releases the reference before restoration.
    unsafe {
        let bytes = std::slice::from_raw_parts_mut(storage.pointer(), 4096);
        bytes[..8].copy_from_slice(&0xbeefu64.to_le_bytes());
    }
    storage.end_host_access();
    assert!(wait.ready());
    let start = std::time::Instant::now();
    loop {
        match cpu.run_parallel(&mut engine, &mut view, u64::MAX) {
            SliceOutcome::Syscall { .. } => break,
            SliceOutcome::Yield { .. } if start.elapsed() < std::time::Duration::from_secs(3) => {}
            other => panic!("resume after lease: {other:?}"),
        }
    }
    drop(view);
    memory.commit_parallel().unwrap();
    assert_eq!(word(&memory), 0xbef0);
}

#[test]
#[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
fn host_managed_ranges_keep_canonical_bytes_and_fork_cow() {
    let data = 2 << 20;
    let mut memory = Memory::new(trames_memory::ADDRESS_LIMIT as usize);
    memory.map(CODE, 4096, Permissions::RW).unwrap();
    for (i, w) in [
        0xf9400141u32,
        0x91000421,
        0xf9000141,
        0xd4000001,
        0x17fffffc,
    ]
    .into_iter()
    .enumerate()
    {
        memory
            .write_array(CODE + i as u64 * 4, w.to_le_bytes())
            .unwrap();
    }
    memory.protect(CODE, 4096, Permissions::RX).unwrap();
    memory.map(data, 2 << 20, Permissions::RW).unwrap();
    let mut cpu = Cpu::new(CODE, 0x800000);
    cpu.set_x(10, data);
    cpu.set_x(8, 172);
    let mut engine = Engine::default();
    syscall(&mut cpu, &mut engine, &mut memory);
    assert_eq!(memory.read_array::<8>(data).unwrap(), 1u64.to_le_bytes());
    assert!(memory.resident_bytes() >= 4096);
    let mut child = memory.try_fork().unwrap();
    let mut child_cpu = cpu.clone();
    memory.write_array(data, 100u64.to_le_bytes()).unwrap();
    syscall(&mut cpu, &mut engine, &mut memory);
    syscall(&mut child_cpu, &mut Engine::default(), &mut child);
    assert_eq!(memory.read_array::<8>(data).unwrap(), 101u64.to_le_bytes());
    assert_eq!(child.read_array::<8>(data).unwrap(), 2u64.to_le_bytes());
}

#[test]
#[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
fn fd_backed_native_image_keeps_original_bytes_after_image_release() {
    let image = trames_graphics::Context
        .allocate(trames_graphics::Format::Rgba8, 65, 37)
        .unwrap();
    let info = image.info();
    let len = info.size.next_multiple_of(4096);
    let mapping = image.shared_mapping().unwrap();
    assert!(mapping.file_descriptor().is_some());
    let pointer = mapping.pointer();
    let (mut cpu, mut memory) =
        setup(&[0xf9400141, 0x91000421, 0xf9000141, 0xd4000001, 0x17fffffc]);
    memory.unmap(DATA, 4096).unwrap();
    memory
        .map_backed(
            DATA,
            image.cpu_pages(0, len).unwrap(),
            true,
            Permissions::RW,
            false,
        )
        .unwrap();
    drop(image);
    drop(mapping);
    let mut engine = Engine::default();
    syscall(&mut cpu, &mut engine, &mut memory);
    assert_eq!(unsafe { pointer.cast::<u64>().read_volatile() }, 1);
    let mut child = memory.try_fork().unwrap();
    let mut child_cpu = cpu.clone();
    syscall(&mut child_cpu, &mut Engine::default(), &mut child);
    // External image mappings keep their shared backing across fork.
    assert_eq!(memory.read_array::<8>(DATA).unwrap(), 2u64.to_le_bytes());
    assert_eq!(unsafe { pointer.cast::<u64>().read_volatile() }, 2);
}

#[test]
#[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
fn duplicate_fd_aliases_share_revoke_counts_and_neighbor_pages_stay_live() {
    use std::{os::fd::AsRawFd, sync::Arc};
    use trames_memory::{CpuMemory, SharedMapping};
    let mapping = SharedMapping::anonymous_file(8192, 65536).unwrap();
    let fd = Arc::new(
        mapping
            .file_descriptor()
            .unwrap()
            .try_clone_to_owned()
            .unwrap(),
    );
    let offset = mapping.file_offset().unwrap();
    let raw = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            8192,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd.as_raw_fd(),
            offset as _,
        )
    };
    assert_ne!(raw, libc::MAP_FAILED);
    assert_ne!(raw as usize, mapping.pointer() as usize);
    struct Owner {
        pointer: usize,
        _fd: Arc<std::os::fd::OwnedFd>,
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            unsafe {
                libc::munmap(self.pointer as *mut _, 8192);
            }
        }
    }
    let owner = Owner {
        pointer: raw as usize,
        _fd: fd.clone(),
    };
    let other = unsafe {
        SharedMapping::from_allocation(raw.cast(), 8192, owner).with_file_descriptor(fd, offset)
    }
    .unwrap();
    let words = [0xf9400141, 0x91000421, 0xf9000141, 0xd4000001, 0x17fffffc];
    let (mut cpu, mut memory) = setup(&words);
    memory.map(DATA + 4096, 4096, Permissions::RW).unwrap();
    memory.import_mapping(DATA, 8192, mapping.clone()).unwrap();
    let mut engine = Engine::default();
    syscall(&mut cpu, &mut engine, &mut memory);
    cpu.set_x(10, DATA + 4096);
    syscall(&mut cpu, &mut engine, &mut memory); // retain the neighbor too
    let (mut second_cpu, mut second_memory) = setup(&words);
    second_memory
        .import_mapping(DATA, 4096, other.clone())
        .unwrap();
    let mut second_engine = Engine::default();
    syscall(&mut second_cpu, &mut second_engine, &mut second_memory);
    let mut registry = Registry::default();
    Cpu::prepare_hardware_memory(&mut memory, &mut registry).unwrap();
    let mut view = memory.parallel_view(&mut registry).unwrap();
    view.enable_hardware();
    let a = view.retained_hardware_page(DATA).unwrap().0;
    let mut registry2 = Registry::default();
    Cpu::prepare_hardware_memory(&mut second_memory, &mut registry2).unwrap();
    let mut view2 = second_memory.parallel_view(&mut registry2).unwrap();
    view2.enable_hardware();
    let b = view2.retained_hardware_page(DATA).unwrap().0;
    assert_eq!(a.physical_address(), b.physical_address());
    assert_ne!(a.pointer(), b.pointer());
    a.begin_host_access();
    b.begin_host_access();
    a.end_host_access();
    // The neighbor is independently accessible while the first 4KiB page is borrowed.
    assert!(matches!(
        cpu.run_parallel(&mut engine, &mut view, u64::MAX),
        SliceOutcome::Syscall { .. }
    ));
    cpu.set_x(10, DATA);
    assert!(matches!(
        cpu.run_parallel(&mut engine, &mut view, u64::MAX),
        SliceOutcome::Yield { .. }
    ));
    let wait = Cpu::take_hardware_wait(&mut engine).unwrap();
    assert!(!wait.ready());
    b.end_host_access();
    assert!(wait.ready());
    let begin = std::time::Instant::now();
    loop {
        match cpu.run_parallel(&mut engine, &mut view, u64::MAX) {
            SliceOutcome::Syscall { .. } => break,
            SliceOutcome::Yield { .. } if begin.elapsed() < std::time::Duration::from_secs(2) => {}
            other => panic!("fd alias resume: {other:?}"),
        }
    }
    assert_eq!(u64::from_le_bytes(view.read_array::<8>(DATA).unwrap()), 3);
}

#[test]
#[ignore = "requires real ARM64 Linux KVM and 4KiB host pages"]
fn retained_writes_republish_after_join_and_follow_original_address_space() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use trames_memory::{MemoryError, PageBacking, SharedPage};
    struct Backing {
        bytes: Arc<SharedPage>,
        publications: AtomicUsize,
        identity: u64,
    }
    impl PageBacking for Backing {
        fn parallel_key(&self) -> Option<(u64, u64)> {
            Some((self.identity, 0))
        }
        fn shared_page(&self) -> Option<Arc<SharedPage>> {
            Some(self.bytes.clone())
        }
        fn check(&self, _: u64) -> Result<(), MemoryError> {
            Ok(())
        }
        fn read(&self, offset: usize, out: &mut [u8]) -> Result<(), MemoryError> {
            self.bytes.copy_read(offset, out);
            Ok(())
        }
        fn write(&self, offset: usize, bytes: &[u8]) -> Result<(), MemoryError> {
            self.bytes.copy_write(offset, bytes);
            Ok(())
        }
        fn publish_write(&self, _: usize, _: usize) -> Result<(), MemoryError> {
            self.publications.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        fn sync(&self) -> Result<(), MemoryError> {
            Ok(())
        }
        fn version(&self) -> u64 {
            self.bytes.revision()
        }
        fn writable(&self) -> bool {
            true
        }
        fn next_page(&self, _: u64) -> Result<Arc<dyn PageBacking>, MemoryError> {
            Err(MemoryError::InvalidRange)
        }
    }
    let backing = || {
        Arc::new(Backing {
            bytes: Arc::new(SharedPage::zero()),
            publications: AtomicUsize::new(0),
            identity: trames_memory::new_backing_identity(),
        })
    };
    let original = backing();
    let replacement = backing();
    let code = [0xf9400141, 0x91000421, 0xf9000141, 0xd4000001, 0x17fffffc];
    let (mut first, mut memory) = setup(&code);
    memory.unmap(DATA, 4096).unwrap();
    memory
        .map_backed(DATA, vec![original.clone()], true, Permissions::RW, false)
        .unwrap();
    let (mut second, mut other) = setup(&code);
    let mut engine = Engine::default();
    let mut registry = Registry::default();
    for round in 1..=4 {
        Cpu::prepare_hardware_memory(&mut memory, &mut registry).unwrap();
        let mut view = memory.parallel_view(&mut registry).unwrap();
        view.enable_hardware();
        for _ in 0..3 {
            let start = std::time::Instant::now();
            loop {
                match first.run_parallel(&mut engine, &mut view, u64::MAX) {
                    SliceOutcome::Syscall { .. } => break,
                    SliceOutcome::Yield { .. } if start.elapsed().as_secs() < 3 => {}
                    other => panic!("shared write: {other:?}"),
                }
            }
        }
        drop(view);
        let before = original.publications.load(Ordering::Relaxed);
        memory.commit_parallel().unwrap();
        assert!(original.publications.load(Ordering::Relaxed) > before);
        assert_eq!(word(&memory), round * 3);
        syscall(&mut second, &mut engine, &mut other);
        assert_eq!(word(&other), round);
    }
    memory.protect(DATA, 4096, Permissions::R).unwrap();
    // Reach the read-only original binding without dirty publication or store.
    first.set_pc(CODE + 12);
    let before = original.publications.load(Ordering::Relaxed);
    syscall(&mut first, &mut engine, &mut memory);
    assert_eq!(original.publications.load(Ordering::Relaxed), before);
    memory.unmap(DATA, 4096).unwrap();
    memory
        .map_backed(
            DATA,
            vec![replacement.clone()],
            true,
            Permissions::RW,
            false,
        )
        .unwrap();
    first.set_pc(CODE);
    syscall(&mut first, &mut engine, &mut memory);
    assert_eq!(word(&memory), 1);
    assert!(replacement.publications.load(Ordering::Relaxed) > 0);
    assert_eq!(original.publications.load(Ordering::Relaxed), before);
}
