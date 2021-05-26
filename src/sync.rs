use crate::os::{BorrowHandle, Handle, SystemTick, WeakHandle, CLOSED_HANDLE};
use crate::result::Result;
use crate::svc::{self, Timeout};

use core::sync::atomic::{AtomicU32, Ordering};

use lock_api::{GuardNoSend, RawMutex, RawMutexTimed};

#[repr(u32)]
#[derive(Debug)]
pub enum ResetType {
    OneShot = 0,
    Sticky,
    Pulse,
}

#[derive(Debug)]
pub struct Event {
    handle: Handle,
}

impl Event {
    pub fn new(reset_type: ResetType) -> Result<Self> {
        let handle = svc::create_event(reset_type)?;
        Ok(Self { handle })
    }

    pub fn borrow_handle(&self) -> WeakHandle {
        self.handle.handle()
    }

    pub fn wait(&self, timeout: Timeout) -> Result<()> {
        svc::wait_synchronization(self.borrow_handle(), timeout)
    }

    pub fn clear(&self) -> Result<()> {
        svc::clear_event(self.borrow_handle())
    }

    pub fn signal(&self) -> Result<()> {
        svc::signal_event(self.borrow_handle())
    }
}

#[derive(Debug)]
struct AtomicHandle(AtomicU32);

impl AtomicHandle {
    const fn from_handle(handle: Handle) -> Self {
        Self(AtomicU32::new(handle.leak()))
    }

    const fn new_closed() -> Self {
        Self::from_handle(Handle::new_closed())
    }

    #[inline]
    unsafe fn get_or_init<F: FnOnce() -> Handle>(&self, init: F) -> WeakHandle {
        let current = self.0.load(Ordering::Acquire);

        let raw_handle = if current == CLOSED_HANDLE {
            let new_handle = init().leak();
            match self.0.compare_exchange(
                CLOSED_HANDLE,
                new_handle,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(new_handle) => new_handle,
                Err(old_handle) => {
                    let _ = svc::close_handle(WeakHandle::new(new_handle)).ok();
                    old_handle
                }
            }
        } else {
            current
        };

        WeakHandle::new(raw_handle)
    }
}

impl Drop for AtomicHandle {
    fn drop(&mut self) {
        let raw_handle = core::mem::replace(self.0.get_mut(), CLOSED_HANDLE);
        let handle = unsafe { Handle::new(raw_handle) };

        drop(handle)
    }
}

impl Default for AtomicHandle {
    fn default() -> Self {
        Self::new_closed()
    }
}

impl From<Handle> for AtomicHandle {
    fn from(handle: Handle) -> Self {
        Self::from_handle(handle)
    }
}

#[derive(Debug)]
pub struct OsMutex {
    handle: AtomicHandle,
}

pub type Mutex<T> = lock_api::Mutex<OsMutex, T>;
pub type MutexGuard<'a, T> = lock_api::MutexGuard<'a, OsMutex, T>;

impl BorrowHandle for AtomicHandle {
    fn borrow_handle<'a>(&'a self) -> WeakHandle<'a> {
        let raw_handle = self.0.load(Ordering::SeqCst);
        WeakHandle::new(raw_handle)
    }
}

impl OsMutex {
    pub fn new() -> Result<Self> {
        const INITIALLY_LOCKED: bool = false;
        let handle = svc::create_mutex(INITIALLY_LOCKED)?.into();

        Ok(Self { handle })
    }

    pub unsafe fn lock(&self, timeout: Timeout) -> Result<()> {
        svc::wait_synchronization(self.handle.borrow_handle(), timeout)?;
        Ok(())
    }

    pub unsafe fn unlock(&self) -> Result<()> {
        svc::release_mutex(self.handle.borrow_handle())?;
        Ok(())
    }

    pub fn destroy(&mut self) -> Result<()> {
        todo!()
        // self.handle.close()
    }

    fn get(&self) -> WeakHandle {
        unsafe {
            self.handle.get_or_init(move || {
                const INITIALLY_LOCKED: bool = true;
                svc::create_mutex(INITIALLY_LOCKED).expect("Failed to create new mutex")
            })
        }
    }
}

unsafe impl RawMutex for OsMutex {
    const INIT: Self = Self {
        handle: AtomicHandle::new_closed(),
    };

    type GuardMarker = GuardNoSend;

    fn lock(&self) {
        let handle = self.get();
        svc::wait_synchronization(handle, Timeout::forever())
            .expect("Failed to lock mutex with infinite timeout")
    }

    fn try_lock(&self) -> bool {
        let handle = self.get();
        svc::wait_synchronization(handle, Timeout::none()).is_ok()
    }

    unsafe fn unlock(&self) {
        let handle = self.get();

        svc::release_mutex(handle).expect("Failed to unlock mutex")
    }
}

unsafe impl RawMutexTimed for OsMutex {
    type Duration = Timeout;
    type Instant = SystemTick;

    fn try_lock_for(&self, timeout: Self::Duration) -> bool {
        let handle = self.get();
        svc::wait_synchronization(handle, timeout).is_ok()
    }

    fn try_lock_until(&self, deadline: Self::Instant) -> bool {
        let now = SystemTick::now();
        let timeout = Timeout::from_nanoseconds((deadline.count() - now.count()).max(0) as i64);
        self.try_lock_for(timeout)
    }
}

#[derive(Debug)]
#[repr(u32)]
pub enum ArbitrationType {
    Signal = 0,
    WaitIfLessThan = 1,
    DecrementAndWaitIfLessThan = 2,
    WaitIfLessThanTimeout = 3,
    DecrementAndWaitIfLessThanTimeout = 4,
}

impl svc::IntoRegister for ArbitrationType {
    type Register = u32;
    unsafe fn into_register(self) -> Self::Register {
        self as Self::Register
    }
}

#[derive(Debug)]
struct AddressArbiter {
    arbiter: AtomicHandle,
}

impl AddressArbiter {
    pub fn new() -> Result<Self> {
        let arbiter = svc::create_address_arbiter()?;
        Ok(Self {
            arbiter: AtomicHandle::from_handle(arbiter),
        })
    }

    fn arbitrate<T: Sized>(
        &self,
        address: &T,
        arbitration_type: ArbitrationType,
        value: i32,
        timeout: Timeout,
    ) -> Result<()> {
        svc::arbitrate_address(
            self.arbiter.borrow_handle(),
            address as *const T as usize,
            arbitration_type,
            value,
            timeout,
        )
    }

    fn wake_up<T: Sized>(&self, address: &T, num_waiters: usize, timeout: Timeout) -> Result<()> {
        self.arbitrate(
            address,
            ArbitrationType::Signal,
            num_waiters as i32,
            timeout,
        )
    }

    fn wake_up_all<T: Sized>(&self, address: &mut T, timeout: Timeout) -> Result<()> {
        self.arbitrate(address, ArbitrationType::Signal, -1, timeout)
    }

    fn wait_if_less_than<T: Ord + Sized + Into<i32>>(&self, address: &T, value: T) -> Result<()> {
        self.arbitrate(
            address,
            ArbitrationType::WaitIfLessThan,
            value.into(),
            Timeout::none(),
        )
    }
}

pub mod spin {
    use crate::result::Result;
    use crate::svc::Timeout;

    use ::spin::{Lazy, RwLock};

    use super::AddressArbiter;

    static ARBITER: Lazy<AddressArbiter> =
        Lazy::new(move || AddressArbiter::new().expect("Could not initialize address arbiter"));

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
    #[repr(i32)]
    enum State {
        Cleared = 0,
        Signaled = 1,
    }

    impl Into<i32> for State {
        fn into(self) -> i32 {
            self as i32
        }
    }

    #[derive(Debug)]
    pub struct StickyEvent(RwLock<State>);

    impl StickyEvent {
        pub const fn new() -> Self {
            Self(RwLock::new(State::Cleared))
        }

        pub fn signal(&self) -> Result<()> {
            let lock = self.0.upgradeable_read();
            match *lock {
                State::Signaled => Ok(()),
                State::Cleared => {
                    let mut lock = lock.upgrade();
                    let state = &mut *lock;
                    *state = State::Signaled;
                    ARBITER.wake_up_all(state, Timeout::forever())?;
                    Ok(())
                }
            }
        }

        pub fn clear(&self) {
            let lock = self.0.upgradeable_read();
            match *lock {
                State::Cleared => {}
                State::Signaled => {
                    *lock.upgrade() = State::Cleared;
                }
            }
        }

        pub fn wait(&self) {
            let state = &*self.0.read();
            match state {
                State::Cleared => {
                    ARBITER.wait_if_less_than(state, State::Signaled).ok();
                }
                State::Signaled => {}
            }
        }

        pub fn try_wait(&self) -> core::result::Result<(), ()> {
            let state = self.0.read();

            match *state {
                State::Cleared => Err(()),
                State::Signaled => Ok(()),
            }
        }
    }
}