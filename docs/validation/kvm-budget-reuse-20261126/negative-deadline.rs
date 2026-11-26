// SPDX-License-Identifier: GPL-3.0-or-later
//! Retained vCPU-local Linux timer. No helper thread and no polling.
use crate::{Error, Result};
use std::{
    cell::Cell,
    marker::PhantomData,
    ptr,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
    time::Duration,
};

#[derive(Clone, Copy)]
struct Active {
    pointer: *mut u8,
    token: usize,
    timer: libc::timer_t,
}
thread_local! {
    static ACTIVE: Cell<Option<Active>> = const { Cell::new(None) };
}
static NEXT: AtomicUsize = AtomicUsize::new(1);
static SIGNAL: OnceLock<std::result::Result<i32, String>> = OnceLock::new();

unsafe extern "C" fn handler(_: i32, info: *mut libc::siginfo_t, _: *mut libc::c_void) {
    if info.is_null() {
        return;
    }
    // The identity belongs to the retained timer, not a particular arm. A late
    // notification from a destroyed vCPU cannot target a new run mapping.
    let token = unsafe {
        if (*info).si_code == libc::SI_TKILL {
            0
        } else if (*info).si_code != libc::SI_TIMER {
            return;
        } else {
            (*info).si_value().sival_ptr as usize
        }
    };
    let _ = ACTIVE.try_with(|active| {
        let Some(active) = active.get() else { return };
        if token != 0 && token != active.token {
            return;
        }
        if token != 0 {
            // Rearming need not remove an already queued expiry. Ignore an old
            // expiry while this timer still has time left. POSIX timer_gettime
            // is async-signal-safe; this query is only on actual timer delivery,
            // never on ordinary SVC entry. Preserve interrupted host errno.
            let expired = unsafe {
                let errno = libc::__errno_location();
                let saved = *errno;
                let mut remaining: libc::itimerspec = std::mem::zeroed();
                let result = libc::timer_gettime(active.timer, &mut remaining);
                *errno = saved;
                result != 0 || (remaining.it_value.tv_sec == 0 && remaining.it_value.tv_nsec == 0)
            };
            if !expired {
                return;
            }
        }
        // SAFETY: arm blocks signal delivery while publishing this stationary
        // mapping; Deadline blocks/drains delivery before clearing the pointer.
        unsafe { active.pointer.write_volatile(1) };
    });
}
fn reserved_signal() -> Result<i32> {
    SIGNAL
        .get_or_init(|| {
            let signal = libc::SIGRTMIN() + 4;
            let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
            if unsafe { libc::sigaction(signal, ptr::null(), &mut old) } != 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            if old.sa_sigaction != libc::SIG_DFL {
                return Err(format!(
                    "reserved deadline signal {signal} already has an owner"
                ));
            }
            let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
            // An assignment budget can expire during a short host syscall. Restart
            // restartable host I/O instead of leaking our private tick as EINTR.
            // KVM immediate_exit and nonrestartable waits still return promptly.
            action.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
            action.sa_sigaction = handler as *const () as usize;
            unsafe { libc::sigemptyset(&mut action.sa_mask) };
            if unsafe { libc::sigaction(signal, &action, ptr::null_mut()) } != 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(signal)
        })
        .clone()
        .map_err(|e| Error::context("install KVM deadline signal", e))
}

/// Creator-thread resource, retained for the vCPU lifetime. Identity allocation
/// and gettid/timer_create occur only here; normal arms reuse the same timer.
pub(crate) struct Timer {
    timer: libc::timer_t,
    signal: i32,
    token: usize,
    _thread_affine: PhantomData<Rc<()>>,
}
impl Timer {
    pub(crate) fn new() -> Result<Self> {
        let signal = reserved_signal()?;
        let token = NEXT.fetch_add(1, Ordering::Relaxed);
        let mut event: libc::sigevent = unsafe { std::mem::zeroed() };
        event.sigev_notify = libc::SIGEV_THREAD_ID;
        event.sigev_signo = signal;
        event.sigev_notify_thread_id = unsafe { libc::syscall(libc::SYS_gettid) } as i32;
        event.sigev_value.sival_ptr = token as *mut libc::c_void;
        let mut timer: libc::timer_t = unsafe { std::mem::zeroed() };
        if unsafe { libc::timer_create(libc::CLOCK_MONOTONIC, &mut event, &mut timer) } != 0 {
            return Err(Error::context(
                "create vCPU deadline",
                std::io::Error::last_os_error(),
            ));
        }
        Ok(Self {
            timer,
            signal,
            token,
            _thread_affine: PhantomData,
        })
    }
    /// # Safety
    /// This timer and immediate_exit outlive the returned guard on the creator
    /// thread. The mapping stays stationary/writable and has no Rust reference
    /// while KVM_RUN or its signal handler can access the byte.
    pub(crate) unsafe fn arm(
        &self,
        immediate_exit: *mut u8,
        duration: Duration,
    ) -> Result<Deadline> {
        if ACTIVE.with(|a| a.get().is_some()) {
            return Err(Error("nested KVM deadline on the same thread".into()));
        }
        let mut mask: libc::sigset_t = unsafe { std::mem::zeroed() };
        let mut old_mask: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigemptyset(&mut mask);
            libc::sigaddset(&mut mask, self.signal);
        }
        let error = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &mask, &mut old_mask) };
        if error != 0 {
            return Err(Error::context(
                "block deadline signal",
                std::io::Error::from_raw_os_error(error),
            ));
        }
        let guard = Deadline {
            timer: self.timer,
            signal: self.signal,
            old_mask,
            _thread_affine: PhantomData,
        };
        unsafe { immediate_exit.write_volatile(0) };
        ACTIVE.with(|a| {
            a.set(Some(Active {
                pointer: immediate_exit,
                token: self.token,
                timer: self.timer,
            }))
        });
        let spec = libc::itimerspec {
            it_interval: libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: libc::timespec {
                tv_sec: duration.as_secs() as _,
                tv_nsec: duration.subsec_nanos().max(1) as _,
            },
        };
        if unsafe { libc::timer_settime(self.timer, 0, &spec, ptr::null_mut()) } != 0 {
            return Err(Error::context(
                "arm thread deadline",
                std::io::Error::last_os_error(),
            ));
        }
        let mut running_mask = guard.old_mask;
        unsafe { libc::sigdelset(&mut running_mask, self.signal) };
        let error =
            unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &running_mask, ptr::null_mut()) };
        if error != 0 {
            return Err(Error::context(
                "unblock deadline",
                std::io::Error::from_raw_os_error(error),
            ));
        }
        Ok(guard)
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        // SAFETY: every guard has ended. Pending notifications retain this
        // retired identity and cannot target another timer's active mapping.
        unsafe { libc::timer_delete(self.timer) };
    }
}
pub(crate) struct Deadline {
    timer: libc::timer_t,
    signal: i32,
    old_mask: libc::sigset_t,
    _thread_affine: PhantomData<Rc<()>>,
}
impl Drop for Deadline {
    fn drop(&mut self) {
        // Same creator thread. Stop delivery before disarming and clearing the
        // mapping; restore the original signal mask before returning to runtime.
        // A queued expiry left behind by a simultaneous kick is harmless: its
        // timer identity and current expiration are checked by the handler.
        unsafe {
            let mut mask: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut mask);
            libc::sigaddset(&mut mask, self.signal);
            libc::pthread_sigmask(libc::SIG_BLOCK, &mask, ptr::null_mut());
            let zero: libc::itimerspec = std::mem::zeroed();
            libc::timer_settime(self.timer, 0, &zero, ptr::null_mut());
            ACTIVE.with(|a| a.set(None));
            let wait = libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            libc::sigtimedwait(&mask, ptr::null_mut(), &wait);
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.old_mask, ptr::null_mut());
        }
    }
}

/// A retained kick handle never accesses the run mapping on a foreign thread.
/// The mutex covers only thread-identity retirement and pthread_kill, never RUN.
#[cfg(target_arch = "aarch64")]
pub(crate) struct Interrupt {
    owner: std::sync::Mutex<Option<libc::pthread_t>>,
    requested: std::sync::atomic::AtomicBool,
    signal: i32,
}
#[cfg(target_arch = "aarch64")]
impl Interrupt {
    pub fn new() -> Result<std::sync::Arc<Self>> {
        Ok(std::sync::Arc::new(Self {
            owner: std::sync::Mutex::new(Some(unsafe { libc::pthread_self() })),
            requested: std::sync::atomic::AtomicBool::new(false),
            signal: reserved_signal()?,
        }))
    }
    pub fn take_request(&self) -> bool {
        self.requested.swap(false, Ordering::AcqRel)
    }
    pub fn retire(&self) {
        *self.owner.lock().unwrap() = None;
    }
}
#[cfg(target_arch = "aarch64")]
impl trames_execution::ExecutionInterrupt for Interrupt {
    fn request_exit(&self) {
        let owner = self.owner.lock().unwrap();
        if let Some(thread) = *owner {
            self.requested.store(true, Ordering::Release);
            // The retained live owner prevents pthread identifier reuse until
            // this call completes. The thread-local signal handler owns the byte.
            unsafe {
                libc::pthread_kill(thread, self.signal);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Mask(libc::sigset_t);
    impl Drop for Mask {
        fn drop(&mut self) {
            unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &self.0, ptr::null_mut()) };
        }
    }
    fn block(signal: i32) -> Mask {
        unsafe {
            let mut set = std::mem::zeroed();
            let mut old = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::sigaddset(&mut set, signal);
            assert_eq!(libc::pthread_sigmask(libc::SIG_BLOCK, &set, &mut old), 0);
            Mask(old)
        }
    }
    #[test]
    fn kernel_timer_interrupts_a_blocking_thread_without_a_helper() {
        let timer = Timer::new().unwrap();
        let mut byte = 0u8;
        let pointer = &raw mut byte;
        let _guard = unsafe { timer.arm(pointer, Duration::from_millis(10)) }.unwrap();
        let wait = libc::timespec {
            tv_sec: 1,
            tv_nsec: 0,
        };
        assert_eq!(unsafe { libc::nanosleep(&wait, ptr::null_mut()) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::EINTR)
        );
        assert_eq!(unsafe { pointer.read_volatile() }, 1);
    }
    #[test]
    fn private_tick_restarts_host_pipe_io() {
        let timer = Timer::new().unwrap();
        let mut byte = 0u8;
        let pointer = &raw mut byte;
        let mut fds = [-1; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            assert_eq!(unsafe { libc::write(fds[1], b"x".as_ptr().cast(), 1) }, 1);
            assert_eq!(unsafe { libc::close(fds[1]) }, 0);
        });
        let guard = unsafe { timer.arm(pointer, Duration::from_millis(5)) }.unwrap();
        let mut value = 0u8;
        assert_eq!(unsafe { libc::read(fds[0], (&raw mut value).cast(), 1) }, 1);
        assert_eq!(value, b'x');
        assert_eq!(unsafe { pointer.read_volatile() }, 1);
        drop(guard);
        assert_eq!(unsafe { libc::close(fds[0]) }, 0);
        writer.join().unwrap();
    }
    #[test]
    fn explicit_kick_interrupts_host_pipe_io() {
        let timer = Timer::new().unwrap();
        let mut byte = 0u8;
        let pointer = &raw mut byte;
        let mut fds = [-1; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let owner = unsafe { libc::pthread_self() };
        let signal = timer.signal;
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            assert_eq!(unsafe { libc::pthread_kill(owner, signal) }, 0);
            std::thread::sleep(Duration::from_millis(20));
            assert_eq!(unsafe { libc::write(fds[1], b"x".as_ptr().cast(), 1) }, 1);
            assert_eq!(unsafe { libc::close(fds[1]) }, 0);
        });
        let guard = unsafe { timer.arm(pointer, Duration::from_secs(60)) }.unwrap();
        let mut value = 0u8;
        assert_eq!(unsafe { libc::read(fds[0], (&raw mut value).cast(), 1) }, -1);
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::EINTR));
        assert_eq!(unsafe { pointer.read_volatile() }, 1);
        sender.join().unwrap();
        assert_eq!(unsafe { libc::close(fds[0]) }, 0);
        drop(guard);
    }
    #[test]
    fn queued_expiry_cannot_cancel_a_fresh_arm_of_the_same_timer() {
        let timer = Timer::new().unwrap();
        let _mask = block(timer.signal);
        // Queue a real old kernel expiry while delivery is blocked and no KVM
        // mapping is active. Rearming must not interpret it as the new deadline.
        let old = libc::itimerspec {
            it_interval: libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: libc::timespec {
                tv_sec: 0,
                tv_nsec: 1_000_000,
            },
        };
        assert_eq!(
            unsafe { libc::timer_settime(timer.timer, 0, &old, ptr::null_mut()) },
            0
        );
        std::thread::sleep(Duration::from_millis(20));
        let mut pending: libc::sigset_t = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::sigpending(&mut pending) }, 0);
        assert_eq!(unsafe { libc::sigismember(&pending, timer.signal) }, 1);
        let mut byte = 0u8;
        let pointer = &raw mut byte;
        let _guard = unsafe { timer.arm(pointer, Duration::from_secs(1)) }.unwrap();
        assert_eq!(unsafe { pointer.read_volatile() }, 0);
        let wait = libc::timespec {
            tv_sec: 2,
            tv_nsec: 0,
        };
        assert_eq!(unsafe { libc::nanosleep(&wait, ptr::null_mut()) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::EINTR)
        );
        assert_eq!(unsafe { pointer.read_volatile() }, 1);
    }
    #[test]
    fn disarming_a_guard_keeps_its_original_byte_untouched() {
        let timer = Timer::new().unwrap();
        let mut byte = 0u8;
        let pointer = &raw mut byte;
        let guard = unsafe { timer.arm(pointer, Duration::from_secs(60)) }.unwrap();
        drop(guard);
        let mut remaining: libc::itimerspec = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::timer_gettime(timer.timer, &mut remaining) },
            0
        );
        assert_eq!(
            (remaining.it_value.tv_sec, remaining.it_value.tv_nsec),
            (0, 0)
        );
        assert_eq!(unsafe { pointer.read_volatile() }, 0);
    }
    #[test]
    fn a_real_kick_interrupts_even_a_future_deadline() {
        let timer = Timer::new().unwrap();
        let mut byte = 0u8;
        let pointer = &raw mut byte;
        let _guard = unsafe { timer.arm(pointer, Duration::from_secs(60)) }.unwrap();
        assert_eq!(
            unsafe { libc::pthread_kill(libc::pthread_self(), timer.signal) },
            0
        );
        assert_eq!(unsafe { pointer.read_volatile() }, 1);
    }
    #[test]
    fn a_retired_timer_notification_cannot_touch_a_different_vcpu() {
        let retired = Timer::new().unwrap();
        let _mask = block(retired.signal);
        let expiration = libc::itimerspec {
            it_interval: libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: libc::timespec {
                tv_sec: 0,
                tv_nsec: 1_000_000,
            },
        };
        assert_eq!(
            unsafe { libc::timer_settime(retired.timer, 0, &expiration, ptr::null_mut()) },
            0
        );
        let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigemptyset(&mut set);
            libc::sigaddset(&mut set, retired.signal);
        }
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let wait = libc::timespec {
            tv_sec: 2,
            tv_nsec: 0,
        };
        assert_eq!(
            unsafe { libc::sigtimedwait(&set, &mut info, &wait) },
            retired.signal
        );
        assert_eq!(info.si_code, libc::SI_TIMER);
        drop(retired);
        let timer = Timer::new().unwrap();
        let mut byte = 0u8;
        let pointer = &raw mut byte;
        let _guard = unsafe { timer.arm(pointer, Duration::from_secs(60)) }.unwrap();
        // Replay the real kernel-produced notification after retirement, even
        // on kernels that discard pending notifications in timer_delete.
        unsafe { handler(timer.signal, &mut info, ptr::null_mut()) };
        assert_eq!(unsafe { pointer.read_volatile() }, 0);
        assert_eq!(
            unsafe { libc::pthread_kill(libc::pthread_self(), timer.signal) },
            0
        );
        assert_eq!(unsafe { pointer.read_volatile() }, 1);
    }
    #[test]
    fn nested_arm_rejection_preserves_the_original_notification_target() {
        let timer = Timer::new().unwrap();
        let mut first = 0u8;
        let mut second = 0u8;
        let original = &raw mut first;
        let rejected = &raw mut second;
        let guard = unsafe { timer.arm(original, Duration::from_secs(60)) }.unwrap();
        assert!(unsafe { timer.arm(rejected, Duration::from_secs(60)) }.is_err());
        assert_eq!(
            unsafe { libc::pthread_kill(libc::pthread_self(), timer.signal) },
            0
        );
        assert_eq!(unsafe { original.read_volatile() }, 1);
        assert_eq!(unsafe { rejected.read_volatile() }, 0);
        drop(guard);
        let _guard = unsafe { timer.arm(rejected, Duration::from_secs(60)) }.unwrap();
        assert_eq!(
            unsafe { libc::pthread_kill(libc::pthread_self(), timer.signal) },
            0
        );
        assert_eq!(unsafe { rejected.read_volatile() }, 1);
    }
}
