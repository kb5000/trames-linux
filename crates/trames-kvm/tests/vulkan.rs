// SPDX-License-Identifier: GPL-3.0-or-later
#![cfg(all(target_os = "linux", target_arch = "aarch64"))]
//! Real host Vulkan command execution over the same original bytes as KVM.
use std::time::{Duration, Instant};
use trames_engine::{Cpu as _, SliceOutcome};
use trames_execution::Registers;
use trames_kvm::kernel::{Cpu, Engine};
use trames_memory::{Memory, Permissions};
use trames_vulkan::{Backend, Invocation};
const PACKET: u64 = 0x1000;
const OUT: u64 = 0x2000;
const DATA: u64 = 0x401000;
const CODE: u64 = 0x801000;
fn call(b: &mut Backend, m: &mut Memory, op: u64, args: &[u64]) {
    m.commit_parallel().unwrap();
    let mut packet = [0; 128];
    for (i, value) in args.iter().enumerate() {
        packet[i * 8..i * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
    m.write(PACKET, &packet).unwrap();
    assert!(
        matches!(
            b.invoke(m, op, PACKET, 128).unwrap(),
            Invocation::Complete(0)
        ),
        "Vulkan op {op}"
    );
}
fn word(m: &Memory, at: u64) -> u64 {
    u64::from_le_bytes(m.read_array(at).unwrap())
}
fn wait(b: &mut Backend, m: &mut Memory, dev: u64, fence: u64) {
    let began = Instant::now();
    loop {
        let mut packet = [0; 128];
        packet[..8].copy_from_slice(&dev.to_le_bytes());
        packet[8..16].copy_from_slice(&fence.to_le_bytes());
        m.write(PACKET, &packet).unwrap();
        match b.invoke(m, 51, PACKET, 128).unwrap() {
            Invocation::Complete(0) => break,
            Invocation::Complete(1) if began.elapsed() < Duration::from_secs(10) => {
                std::thread::sleep(Duration::from_millis(1))
            }
            other => panic!("fence {other:?}"),
        }
    }
}
#[test]
#[ignore = "requires ARM64 KVM, 4KiB pages and Vulkan external_memory_host"]
fn vulkan_host_buffer_and_kvm_share_original_bytes_without_shadow_transfers() {
    let mut b = Backend::new().with_native_pointer_imports(false);
    let mut m = Memory::default();
    assert_eq!(DATA % 16384, 4096);
    assert_eq!(CODE % 16384, 4096);
    m.map(PACKET, 0x4000, Permissions::RW).unwrap();
    m.map(DATA, 4 << 20, Permissions::RW).unwrap();
    for guard in [DATA - 4096, DATA + (4 << 20)] {
        m.map(guard, 4096, Permissions::RW).unwrap();
        m.write(guard, &[0x6d; 4096]).unwrap();
        m.protect(guard, 4096, Permissions::R).unwrap();
    }
    m.map(CODE, 4096, Permissions::RW).unwrap();
    for (i, instruction) in [
        0xb9400141u32,
        0x11000421,
        0xb9000141,
        0xd4000001,
        0x17fffffc,
    ]
    .iter()
    .enumerate()
    {
        m.write_array(CODE + i as u64 * 4, instruction.to_le_bytes())
            .unwrap();
    }
    m.protect(CODE, 4096, Permissions::RX).unwrap();
    call(&mut b, &mut m, 1, &[OUT]);
    let instance = word(&m, OUT);
    m.write_array(0x2080, 1u32.to_le_bytes()).unwrap();
    call(&mut b, &mut m, 3, &[instance, 0x2080, OUT]);
    let physical = word(&m, OUT);
    call(&mut b, &mut m, 8, &[physical, 0, OUT]);
    let dev = word(&m, OUT);
    call(&mut b, &mut m, 200, &[dev, OUT]);
    assert_ne!(word(&m, OUT) & 1, 0, "driver must support host imports");
    call(&mut b, &mut m, 10, &[dev, 0, 0, OUT]);
    let queue = word(&m, OUT);
    call(&mut b, &mut m, 11, &[dev, 2 << 20, 3, OUT]);
    let buffer = word(&m, OUT);
    call(&mut b, &mut m, 13, &[dev, buffer, 0x2100]);
    let size = word(&m, 0x2100);
    let bits = word(&m, 0x2110) as u32;
    assert_eq!(size, 2 << 20);
    call(&mut b, &mut m, 5, &[physical, 0x2500]);
    let ty = (0..word(&m, 0x2500) as u32)
        .find(|i| {
            bits & (1 << i) != 0
                && u32::from_le_bytes(m.read_array(0x2504 + *i as u64 * 8).unwrap()) & 6 == 6
        })
        .unwrap();
    call(
        &mut b,
        &mut m,
        14,
        &[dev, size, ty as u64, OUT, 0, 1, buffer],
    );
    let memory = word(&m, OUT);
    call(&mut b, &mut m, 16, &[dev, buffer, memory, 0]);
    call(
        &mut b,
        &mut m,
        17,
        &[dev, memory, 0, size, DATA, 0x5348_4152_4544],
    );
    call(&mut b, &mut m, 34, &[dev, 0, 2, OUT]);
    let pool = word(&m, OUT);
    call(&mut b, &mut m, 36, &[dev, pool, OUT]);
    let cmd = word(&m, OUT);
    call(&mut b, &mut m, 48, &[dev, 0, OUT]);
    let fence = word(&m, OUT);
    call(&mut b, &mut m, 38, &[dev, cmd, 0]);
    call(&mut b, &mut m, 46, &[dev, cmd, buffer, 0, 4096, 0x10203040]);
    call(
        &mut b,
        &mut m,
        44,
        &[dev, cmd, 4096, 16384, 4096, 8192, 0, 0, 6],
    );
    call(&mut b, &mut m, 39, &[dev, cmd]);
    m.write_array(0x3000, cmd.to_le_bytes()).unwrap();
    for (i, value) in [0u64, 0, 0, 0x3000, 1, 0, 0].iter().enumerate() {
        m.write_array(0x3100 + i as u64 * 8, value.to_le_bytes())
            .unwrap();
    }
    let before = b.transfers;
    call(&mut b, &mut m, 56, &[dev, queue, 0x3100, 1, fence]);
    wait(&mut b, &mut m, dev, fence);
    assert_eq!(u32::from_le_bytes(m.read_array(DATA).unwrap()), 0x10203040);
    let mut cpu = Cpu::new(CODE, 0xa00000);
    cpu.set_x(10, DATA);
    cpu.set_x(8, 172);
    let mut engine = Engine::default();
    let began = Instant::now();
    loop {
        match cpu.run(&mut engine, &mut m, u64::MAX) {
            SliceOutcome::Syscall { .. } => break,
            SliceOutcome::Yield { .. } if began.elapsed() < Duration::from_secs(3) => {}
            other => panic!("KVM shared buffer: {other:?}"),
        }
    }
    assert_eq!(u32::from_le_bytes(m.read_array(DATA).unwrap()), 0x10203041);
    // Reverse direction: native Vulkan reads the word written by original KVM
    // instructions, then copies it to an independent imported buffer.
    call(&mut b, &mut m, 11, &[dev, size, 3, OUT]);
    let copied = word(&m, OUT);
    call(
        &mut b,
        &mut m,
        14,
        &[dev, size, ty as u64, OUT, 0, 1, copied],
    );
    let copied_memory = word(&m, OUT);
    call(&mut b, &mut m, 16, &[dev, copied, copied_memory, 0]);
    call(
        &mut b,
        &mut m,
        17,
        &[dev, copied_memory, 0, size, DATA + size, 0x5348_4152_4544],
    );
    call(&mut b, &mut m, 50, &[dev, fence]);
    call(&mut b, &mut m, 53, &[dev, pool, 0]);
    call(&mut b, &mut m, 38, &[dev, cmd, 0]);
    call(
        &mut b,
        &mut m,
        44,
        &[dev, cmd, 16384, 4096, 16384, 2048, 0, 0, 6],
    );
    m.write(
        0x3200,
        &[0u64, 0, 4096]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    call(&mut b, &mut m, 45, &[dev, cmd, buffer, copied, 0x3200, 1]);
    call(
        &mut b,
        &mut m,
        44,
        &[dev, cmd, 4096, 16384, 4096, 8192, 0, 0, 6],
    );
    call(&mut b, &mut m, 39, &[dev, cmd]);
    call(&mut b, &mut m, 56, &[dev, queue, 0x3100, 1, fence]);
    wait(&mut b, &mut m, dev, fence);
    assert_eq!(
        u32::from_le_bytes(m.read_array(DATA + size).unwrap()),
        0x10203041
    );
    assert_eq!(
        b.transfers.coherent_upload_scanned_bytes,
        before.coherent_upload_scanned_bytes
    );
    assert_eq!(
        b.transfers.coherent_download_scanned_bytes,
        before.coherent_download_scanned_bytes
    );
    call(&mut b, &mut m, 18, &[dev, memory]);
    call(&mut b, &mut m, 9, &[dev]);
    call(&mut b, &mut m, 2, &[instance]);
    // Imported pages retain their original owner after all native API handles close.
    assert_eq!(u32::from_le_bytes(m.read_array(DATA).unwrap()), 0x10203041);
    for guard in [DATA - 4096, DATA + (4 << 20)] {
        assert_eq!(m.read_array::<4096>(guard).unwrap(), [0x6d; 4096]);
        assert!(m.write(guard, &[0x77]).is_err());
    }
    assert!(m.write(CODE, &[0]).is_err());
}
