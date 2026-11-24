// SPDX-License-Identifier: GPL-3.0-or-later
//! Movable task state; hardware engines stay on their creator worker thread.
use trames_engine::{CpuFault, DecodeStats, SliceOutcome};
use trames_execution::{Aarch64State, Features, Registers};
use trames_memory::{
    parallel::{Registry, View},
    Memory, MemoryError,
};

#[derive(Clone, Default)]
pub struct Cpu {
    pub state: Aarch64State,
    vulkan: bool,
    video: bool,
    audio: bool,
    single_step: bool,
}
#[derive(Default)]
pub struct Engine {
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    executor: Option<crate::aarch64::engine::Executor>,
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    registry: Registry,
}
impl Engine {
    pub fn mapped_bytes(&self) -> usize {
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            self.executor.as_ref().map_or(0, |e| e.mapped_bytes())
        }
        #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
        {
            0
        }
    }
    pub fn ram_slots(&self) -> usize {
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            self.executor.as_ref().map_or(0, |e| e.slot_count())
        }
        #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
        {
            0
        }
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    fn prepare(&mut self) -> Result<&mut crate::aarch64::engine::Executor, CpuFault> {
        if self.executor.is_none() {
            self.executor = Some(
                crate::aarch64::engine::Executor::new()
                    .map_err(|e| CpuFault::Host(e.to_string()))?,
            );
        }
        Ok(self.executor.as_mut().unwrap())
    }
}
impl Registers for Cpu {
    fn x(&self, index: usize) -> u64 {
        self.state.x(index)
    }
    fn set_x(&mut self, index: usize, value: u64) {
        self.state.set_x(index, value);
    }
    fn pc(&self) -> u64 {
        self.state.pc
    }
}
impl Cpu {
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    fn outcome(
        &self,
        result: Result<trames_execution::ExecutionExit, crate::aarch64::EngineError>,
        memory: &impl trames_memory::CpuMemory,
    ) -> SliceOutcome {
        use crate::aarch64::EngineError;
        if let Err(error) = &result {
            if std::env::var_os("TRAMES_KVM_TRACE_FAULTS").is_some() {
                eprintln!("trames-kvm: PC={:#x} {error:?}", self.state.pc);
            }
        }
        match result {
            Ok(exit) => match exit.reason {
                trames_execution::ExitReason::Syscall => SliceOutcome::Syscall { instructions: 0 },
                trames_execution::ExitReason::Yield => SliceOutcome::Yield { instructions: 0 },
            },
            Err(EngineError::Host(e)) => SliceOutcome::Fault(CpuFault::Host(e.to_string())),
            Err(EngineError::Memory(
                error @ (MemoryError::BackingIo(95) | MemoryError::InvalidRange),
            )) => SliceOutcome::Fault(CpuFault::Host(format!("KVM memory admission: {error}"))),
            Err(EngineError::Memory(error)) => SliceOutcome::Fault(CpuFault::Memory {
                pc: self.state.pc,
                error,
            }),
            Err(EngineError::Trap { syndrome, address }) => {
                let ec = (syndrome >> 26) & 63;
                if ec == 0
                    && ((self.vulkan && self.x(16) == trames_accel::native::VULKAN_PROFILE)
                        || (self.video && self.x(16) == trames_accel::native::VIDEO_PROFILE)
                        || (self.audio && self.x(16) == trames_accel::native::AUDIO_PROFILE))
                {
                    match memory.fetch_u32(self.state.pc) {
                        Ok(trames_accel::native::INSTRUCTION) => {
                            return SliceOutcome::NativeCall {
                                instructions: 0,
                                pc: self.state.pc,
                                module: self.x(16),
                                function: self.x(17),
                            }
                        }
                        Err(error) => {
                            return SliceOutcome::Fault(CpuFault::Memory {
                                pc: self.state.pc,
                                error,
                            })
                        }
                        _ => {}
                    }
                }
                match ec {
                    0 | 0x18 | 0x1d => SliceOutcome::Fault(CpuFault::Trap {
                        pc: self.state.pc,
                        instruction: 0,
                        signal: 4,
                        code: 1,
                    }),
                    0x3c => SliceOutcome::Fault(CpuFault::Trap {
                        pc: self.state.pc,
                        instruction: 0,
                        signal: 5,
                        code: 1,
                    }),
                    0x22 | 0x26 => SliceOutcome::Fault(CpuFault::Memory {
                        pc: self.state.pc,
                        error: MemoryError::Alignment(if ec == 0x26 {
                            self.state.sp
                        } else {
                            self.state.pc
                        }),
                    }),
                    _ => SliceOutcome::Fault(CpuFault::Host(format!(
                        "unhandled KVM exception ESR={syndrome:#x} FAR={address:#x}"
                    ))),
                }
            }
        }
    }
    fn parallel(
        &mut self,
        engine: &mut Engine,
        memory: &mut View,
        quantum: std::time::Duration,
    ) -> SliceOutcome {
        if self.single_step {
            return SliceOutcome::Fault(CpuFault::Host(
                "KVM guest single-step is not implemented".into(),
            ));
        }
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            let executor = match engine.prepare() {
                Ok(e) => e,
                Err(e) => return SliceOutcome::Fault(e),
            };
            let result = executor.run(&mut self.state, memory, quantum);
            self.outcome(result, memory)
        }
        #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
        {
            let _ = (engine, memory, quantum);
            SliceOutcome::Fault(CpuFault::Host(
                "KVM execution requires Linux/aarch64".into(),
            ))
        }
    }
    fn coordinated(
        &mut self,
        engine: &mut Engine,
        memory: &mut Memory,
        registry: &mut Registry,
        quantum: std::time::Duration,
    ) -> SliceOutcome {
        if let Err(error) = <Self as trames_engine::Cpu>::prepare_hardware_memory(memory, registry)
        {
            return SliceOutcome::Fault(CpuFault::Memory {
                pc: self.state.pc,
                error,
            });
        }
        let Some(mut view) = memory.parallel_view(registry) else {
            return SliceOutcome::Fault(CpuFault::Host(
                "runtime rejected KVM memory admission".into(),
            ));
        };
        view.enable_hardware();
        let outcome = self.parallel(engine, &mut view, quantum);
        drop(view);
        if let Err(error) = memory.commit_parallel() {
            return SliceOutcome::Fault(CpuFault::Memory {
                pc: self.state.pc,
                error,
            });
        }
        outcome
    }
}
impl trames_engine::Cpu for Cpu {
    type Engine = Engine;
    const FEATURES: Features = Features {
        hwcap: 3,
        hwcap2: 0,
    };
    const FEATURE_NAMES: &'static str = "fp asimd";
    const EXACT_INSTRUCTIONS: bool = false;
    fn new(pc: u64, sp: u64) -> Self {
        Self {
            state: Aarch64State {
                pc,
                sp,
                ..Default::default()
            },
            ..Default::default()
        }
    }
    fn sp(&self) -> u64 {
        self.state.sp
    }
    fn set_sp(&mut self, v: u64) {
        self.state.sp = v;
    }
    fn set_pc(&mut self, v: u64) {
        self.state.pc = v;
    }
    fn set_tls(&mut self, v: u64) {
        self.state.tpidr_el0 = v;
    }
    fn architectural_state(&self) -> Aarch64State {
        self.state.clone()
    }
    fn restore_architectural_state(&mut self, s: &Aarch64State) {
        self.state.clone_from(s);
    }
    fn set_native_calls(&mut self, v: bool) {
        assert!(!v, "KVM executes original native instructions");
    }
    fn set_vulkan_calls(&mut self, v: bool) {
        self.vulkan = v;
    }
    fn set_video_calls(&mut self, v: bool) {
        self.video = v;
    }
    fn set_audio_calls(&mut self, v: bool) {
        self.audio = v;
    }
    fn set_single_step(&mut self, v: bool) {
        self.single_step = v;
    }
    fn clear_exclusive(&mut self) {}
    fn fault_retired(&self) -> u64 {
        0
    }
    fn decode_stats(&self) -> DecodeStats {
        DecodeStats::default()
    }
    fn execution_interrupt(
        engine: &mut Engine,
    ) -> Result<Option<std::sync::Arc<dyn trames_engine::ExecutionInterrupt>>, CpuFault> {
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            Ok(Some(engine.prepare()?.interrupt()))
        }
        #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
        {
            let _ = engine;
            Err(CpuFault::Host(
                "KVM execution requires Linux/aarch64".into(),
            ))
        }
    }
    fn prepare_hardware_memory(
        memory: &mut Memory,
        registry: &mut Registry,
    ) -> Result<(), MemoryError> {
        registry.use_hardware();
        memory.prepare_hardware()
    }
    fn prepare_hardware_view(view: &mut View) {
        view.enable_hardware();
    }
    fn take_hardware_wait(
        engine: &mut Engine,
    ) -> Option<std::sync::Arc<trames_memory::HardwareWait>> {
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            engine.executor.as_mut().and_then(|e| e.take_wait())
        }
        #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
        {
            let _ = engine;
            None
        }
    }
    fn run(&mut self, engine: &mut Engine, memory: &mut Memory, _: u64) -> SliceOutcome {
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        let mut registry = std::mem::take(&mut engine.registry);
        #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
        let mut registry = Registry::default();
        let result = self.coordinated(
            engine,
            memory,
            &mut registry,
            std::time::Duration::from_millis(2),
        );
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            engine.registry = registry;
        }
        result
    }
    fn run_quiesced(
        &mut self,
        engine: &mut Engine,
        memory: &mut Memory,
        registry: &mut Registry,
        quantum: std::time::Duration,
    ) -> Option<SliceOutcome> {
        Some(self.coordinated(engine, memory, registry, quantum))
    }
    fn run_parallel(&mut self, engine: &mut Engine, memory: &mut View, _: u64) -> SliceOutcome {
        self.parallel(engine, memory, std::time::Duration::from_millis(2))
    }
}
