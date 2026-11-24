// SPDX-License-Identifier: GPL-3.0-or-later
//! One-shot Linux thread-targeted timer. No helper thread and no polling.
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

thread_local! {
    static ACTIVE: Cell<(*mut u8, usize)> = const { Cell::new((ptr::null_mut(), 0)) };
}
static NEXT: AtomicUsize = AtomicUsize::new(1);
static SIGNAL: OnceLock<std::result::Result<i32, String>> = OnceLock::new();

unsafe extern "C" fn handler(_: i32, info: *mut libc::siginfo_t, _: *mut libc::c_void) {
    if info.is_null() {
        return;
    }
    // SAFETY: SA_SIGINFO supplies a live siginfo_t. This reserved signal accepts
    // only our one-shot timers, tagged so late delivery cannot hit a new vCPU.
    let token = unsafe {
        if (*info).si_code != libc::SI_TIMER {
            return;
        }
        (*info).si_value().sival_ptr as usize
    };
    let _ = ACTIVE.try_with(|active| {
        let (pointer, expected) = active.get();
        if !pointer.is_null() && token == expected {
            // SAFETY: arm retains the run mapping and blocks/drains this signal
            // before the pointer is cleared or the vCPU can be destroyed.
            unsafe { pointer.write_volatile(1) };
        }
    });
}
fn reserved_signal() -> Result<i32> {
    SIGNAL
        .get_or_init(|| {
            let signal = libc::SIGRTMIN() + 4;
            // SAFETY: initialized storage for libc output and an empty signal mask.
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
            action.sa_flags = libc::SA_SIGINFO;
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
pub(crate) struct Deadline {
    timer: libc::timer_t,
    signal: i32,
    old_mask: libc::sigset_t,
    _thread_affine: PhantomData<Rc<()>>,
}
impl Deadline {
    /// # Safety
    /// immediate_exit remains stationary writable storage until this guard drops.
    /// The caller executes KVM_RUN only on this thread and holds no reference to
    /// this byte across signal delivery.
    pub(crate) unsafe fn arm(immediate_exit: *mut u8, duration: Duration) -> Result<Self> {
        let signal = reserved_signal()?;
        if ACTIVE.with(|a| !a.get().0.is_null()) {
            return Err(Error("nested KVM deadline on the same thread".into()));
        }
        let mut mask: libc::sigset_t = unsafe { std::mem::zeroed() };
        let mut old_mask: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigemptyset(&mut mask);
            libc::sigaddset(&mut mask, signal);
        }
        let error = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &mask, &mut old_mask) };
        if error != 0 {
            return Err(Error::context(
                "block deadline signal",
                std::io::Error::from_raw_os_error(error),
            ));
        }
        let token = NEXT.fetch_add(1, Ordering::Relaxed);
        let mut event: libc::sigevent = unsafe { std::mem::zeroed() };
        event.sigev_notify = libc::SIGEV_THREAD_ID;
        event.sigev_signo = signal;
        event.sigev_notify_thread_id = unsafe { libc::syscall(libc::SYS_gettid) } as i32;
        event.sigev_value.sival_ptr = token as *mut libc::c_void;
        let mut timer: libc::timer_t = unsafe { std::mem::zeroed() };
        if unsafe { libc::timer_create(libc::CLOCK_MONOTONIC, &mut event, &mut timer) } != 0 {
            let error = std::io::Error::last_os_error();
            unsafe {
                libc::pthread_sigmask(libc::SIG_SETMASK, &old_mask, ptr::null_mut());
            }
            return Err(Error::context("create thread deadline", error));
        }
        let guard = Self {
            timer,
            signal,
            old_mask,
            _thread_affine: PhantomData,
        };
        unsafe { immediate_exit.write_volatile(0) };
        ACTIVE.with(|a| a.set((immediate_exit, token)));
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
        if unsafe { libc::timer_settime(timer, 0, &spec, ptr::null_mut()) } != 0 {
            return Err(Error::context(
                "arm thread deadline",
                std::io::Error::last_os_error(),
            ));
        }
        let mut running_mask = guard.old_mask;
        unsafe {
            libc::sigdelset(&mut running_mask, signal);
        }
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
impl Drop for Deadline {
    fn drop(&mut self) {
        // SAFETY: same creator thread; block delivery before clearing the TLS
        // mapping, delete the one-shot timer, and drain its at-most-one signal.
        unsafe {
            let mut mask: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut mask);
            libc::sigaddset(&mut mask, self.signal);
            libc::pthread_sigmask(libc::SIG_BLOCK, &mask, ptr::null_mut());
            libc::timer_delete(self.timer);
            ACTIVE.with(|a| a.set((ptr::null_mut(), 0)));
            let zero = libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            libc::sigtimedwait(&mask, ptr::null_mut(), &zero);
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.old_mask, ptr::null_mut());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kernel_timer_interrupts_a_blocking_thread_without_a_helper() {
        let mut byte = 0u8;
        let pointer = &raw mut byte;
        let _guard = unsafe { Deadline::arm(pointer, Duration::from_millis(10)) }.unwrap();
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
}
