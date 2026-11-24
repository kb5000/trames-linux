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
        assert!([CODE, CODE + 4].contains(&cpu.pc()));
    }
    assert!(previous > 0);
    drop(first);
    kick.request_exit(); // retained handle is inert after retirement
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
