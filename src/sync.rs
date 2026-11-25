// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

pub mod arbitrate;

use crate::early_debug;
use crate::os::{AsHandle, BorrowedHandle, CLOSED_HANDLE, OwnedHandle, SystemTick};
use crate::result::Result;
use crate::svc::{self, Timeout};

use core::sync::atomic::{AtomicU32, Ordering};

use talc::lock_api::{self, GuardNoSend, RawMutex, RawMutexTimed};

#[repr(u32)]
#[derive(Debug)]
pub enum ResetType {
    OneShot = 0,
    Sticky,
    Pulse,
}

#[derive(Debug)]
#[repr(transparent)]
pub struct Event {
    handle: OwnedHandle,
}

impl Event {
    pub fn new(reset_type: ResetType) -> Result<Self> {
        let handle = svc::create_event(reset_type)?;
        Ok(Self { handle })
    }

    pub unsafe fn from_handle(handle: OwnedHandle) -> Self {
        Self { handle }
    }

    pub fn wait(&self, timeout: Timeout) -> Result<()> {
        svc::wait_synchronization(self.as_handle(), timeout)
    }

    pub fn clear(&self) -> Result<()> {
        svc::clear_event(self.as_handle())
    }

    pub fn signal(&self) -> Result<()> {
        svc::signal_event(self.as_handle())
    }

    pub fn wait_all(events: &[Self], timeout: Timeout) -> Result<()> {
        // SAFETY: Both `Event` and `WeakHandle` have the same layout.
        // Therefore it is safe to transmute &[Event] into &[WeakHandle].
        //
        // There are the following #[repr(transparent)]-chains:
        // * Event -> Handle -> Option<NonZeroU32> (-> u32, niche optimization)
        // * WeakHandle -> u32
        let handles: &[BorrowedHandle] = unsafe { core::mem::transmute(events) };
        svc::wait_synchronization_all(handles, timeout)
    }

    pub fn wait_any(events: &[Self], timeout: Timeout) -> Result<usize> {
        // SAFETY: See comment in Self::wait_all.
        let handles: &[BorrowedHandle] = unsafe { core::mem::transmute(events) };
        svc::wait_synchronization_any(handles, timeout)
    }

    pub fn duplicate(&self) -> Result<Self> {
        let duplicated = svc::duplicate_handle(self.as_handle())?;
        Ok(Self { handle: duplicated })
    }
}

impl AsHandle for Event {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

#[derive(Debug)]
struct AtomicHandle(AtomicU32);

impl AtomicHandle {
    const fn from_handle(handle: OwnedHandle) -> Self {
        Self(AtomicU32::new(handle.leak()))
    }

    const fn new_closed() -> Self {
        Self(AtomicU32::new(CLOSED_HANDLE))
    }

    fn get(&self) -> BorrowedHandle<'_> {
        let raw_handle = self.0.load(Ordering::Acquire);
        BorrowedHandle::new(raw_handle)
    }

    /// Store `handle` if no handle is stored yet, taking ownership of it.
    ///
    /// Returns the stored handle on success.  If a handle is already stored, `handle` is handed
    /// back to the caller unchanged; dropping it closes it.
    fn try_set(
        &self,
        handle: OwnedHandle,
    ) -> core::result::Result<BorrowedHandle<'_>, OwnedHandle> {
        match self.0.compare_exchange(
            CLOSED_HANDLE,
            handle.as_raw_handle(),
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            // Consume the new handle, do not run its destructor.
            Ok(_closed_handle) => Ok(BorrowedHandle::new(handle.leak())),
            Err(_old_handle) => Err(handle),
        }
    }

    #[cold]
    fn init<'handle, F: FnOnce() -> OwnedHandle>(
        &'handle self,
        init: F,
    ) -> BorrowedHandle<'handle> {
        let new_handle: OwnedHandle = init();
        debug_assert!(
            new_handle.is_open(),
            "AtomicHandle::init(): Initializer returned closed handle"
        );

        early_debug!(
            "AtomicHandle::init(new_handle={:08x})",
            new_handle.as_raw_handle()
        );

        match self.try_set(new_handle) {
            Ok(new_handle) => {
                early_debug!(
                    "AtomicHandle::init(new_handle={:08x}): Won the race, emplaced new_handle.",
                    new_handle.handle
                );
                new_handle
            }
            Err(new_handle) => {
                // A stored handle is never replaced, so it is safe to read it again.
                let old_handle = self.get();
                early_debug!(
                    "AtomicHandle::init(new_handle={:08x}, old_handle={:08x}): Lost the race, already initialized",
                    new_handle.as_raw_handle(),
                    old_handle.handle
                );
                drop(new_handle);
                old_handle
            }
        }
    }

    fn get_or_init<F: FnOnce() -> OwnedHandle>(&self, init: F) -> BorrowedHandle<'_> {
        let handle = self.get();

        if handle.is_closed() {
            self.init(init)
        } else {
            handle
        }
    }
}

impl Drop for AtomicHandle {
    fn drop(&mut self) {
        let raw_handle = core::mem::replace(self.0.get_mut(), CLOSED_HANDLE);
        let _ = unsafe { svc::close_handle(raw_handle) };
    }
}

impl Default for AtomicHandle {
    fn default() -> Self {
        Self::new_closed()
    }
}

impl From<OwnedHandle> for AtomicHandle {
    fn from(handle: OwnedHandle) -> Self {
        Self::from_handle(handle)
    }
}

#[derive(Debug)]
pub struct OsMutex {
    handle: AtomicHandle,
}

pub type Mutex<T> = lock_api::Mutex<OsMutex, T>;
pub type MutexGuard<'a, T> = lock_api::MutexGuard<'a, OsMutex, T>;

impl AsHandle for AtomicHandle {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        let raw_handle = self.0.load(Ordering::SeqCst);
        BorrowedHandle::new(raw_handle)
    }
}

impl OsMutex {
    pub fn new() -> Result<Self> {
        const INITIALLY_LOCKED: bool = false;
        let handle = svc::create_mutex(INITIALLY_LOCKED)?.into();

        Ok(Self { handle })
    }

    pub unsafe fn from_handle(handle: OwnedHandle) -> Self {
        Self {
            handle: handle.into(),
        }
    }

    pub unsafe fn lock(&self, timeout: Timeout) -> Result<()> {
        svc::wait_synchronization(self.handle.as_handle(), timeout)?;
        Ok(())
    }

    pub unsafe fn unlock(&self) -> Result<()> {
        svc::release_mutex(self.handle.as_handle())?;
        Ok(())
    }

    pub fn destroy(&mut self) -> Result<()> {
        todo!()
        // self.handle.close()
    }

    fn get(&self) -> BorrowedHandle<'_> {
        self.handle.get_or_init(move || {
            early_debug!("Initializing OsMutex");
            const INITIALLY_LOCKED: bool = false;
            svc::create_mutex(INITIALLY_LOCKED).expect("Failed to create new mutex")
        })
    }
}

unsafe impl RawMutex for OsMutex {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self = Self {
        handle: AtomicHandle::new_closed(),
    };

    type GuardMarker = GuardNoSend;

    fn lock(&self) {
        let handle = self.get();
        svc::wait_synchronization(handle, Timeout::infinite())
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
        self.try_lock_for(Timeout::from_system_tick(SystemTick::until(deadline)))
    }
}

pub mod mem {
    use core::arch::arm::{__dsb, SY};

    #[inline(always)]
    pub fn data_sync_barrier() {
        // SAFETY: ARMv6K implements the full-system CP15 barrier operation.
        unsafe { __dsb(SY) };
    }
}
