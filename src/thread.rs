// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::cell::UnsafeCell;
use core::fmt;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::early_debug;
use crate::os::{AsHandle, OwnedHandle};
use crate::result::Result;
use crate::svc::{self, Timeout};

use alloc::boxed::Box;
use alloc::{self, alloc::Layout};

use log::debug;

unsafe extern "C" fn _ctru_rt_thread_start(argument: usize) {
    early_debug!("We are in _ctru_rt_thread_start(0x{:08x})!", argument);
    let packet = ThreadPacket::from_argument(argument);

    early_debug!("Got a packet: entry_point={:p}", packet.entry_point);

    (packet.entry_point)();

    early_debug!("_ctru_rt_thread_start: Exiting...");

    svc::exit_thread();
}

#[repr(transparent)]
struct ThreadPacket {
    entry_point: Box<dyn FnOnce()>,
}

impl ThreadPacket {
    pub(crate) fn new(entry_point: impl FnOnce() + Send + 'static) -> Box<Self> {
        Box::new(Self {
            entry_point: Box::new(entry_point),
        })
    }

    unsafe fn from_argument(argument: usize) -> Box<ThreadPacket> {
        Box::from_raw(argument as *mut ThreadPacket)
    }

    fn into_argument(packet: Box<ThreadPacket>) -> usize {
        Box::into_raw(packet) as usize
    }
}

#[derive(Debug)]
struct ReturnValue<T> {
    is_set: AtomicBool,
    value: UnsafeCell<MaybeUninit<T>>,
}

impl<T> ReturnValue<T> {
    fn new() -> Self {
        Self {
            is_set: AtomicBool::new(false),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    fn is_set(&self) -> bool {
        self.is_set.load(Ordering::Acquire)
    }

    unsafe fn read(&self) -> Option<T> {
        if !self.is_set() {
            return None;
        }

        let value = unsafe { (*self.value.get()).assume_init_read() };
        Some(value)
    }

    unsafe fn store(&self, return_value: T) {
        (*self.value.get()).write(return_value);
        self.is_set.store(true, Ordering::Release);
    }
}

struct ReturnValuePtr<T>(*mut ReturnValue<T>);
unsafe impl<T> Send for ReturnValuePtr<T> where T: Send + 'static {}

impl<T> Copy for ReturnValuePtr<T> {}
impl<T> Clone for ReturnValuePtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> ReturnValuePtr<T> {
    unsafe fn new(ptr: *mut ReturnValue<T>) -> Self {
        ptr.write(ReturnValue::new());
        Self(ptr)
    }

    unsafe fn store(&self, return_value: T) {
        (*self.0).store(return_value)
    }
}

struct ThreadMemory<T> {
    allocated: *mut u8,
    stack_top: *mut u8,
    return_value: ReturnValuePtr<T>,
    layout: Layout,
}

impl<T> fmt::Debug for ThreadMemory<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ThreadMemory")
            .field("allocate", &self.allocated)
            .field("stack_top", &self.stack_top)
            .field("return_value", &self.return_value.0)
            .field("layout", &self.layout)
            .finish()
    }
}

impl<T> ThreadMemory<T> {
    fn allocate(stack_size: usize) -> Self {
        let layout = Layout::array::<u8>(align_to(stack_size, 8))
            .unwrap()
            .align_to(8)
            .unwrap();
        let stack_size = layout.size();

        let (layout, rv_offset) = layout.extend(Layout::new::<ReturnValue<T>>()).unwrap();
        let layout = layout.pad_to_align();

        let allocated = unsafe { alloc::alloc::alloc(layout) };

        let stack_top = unsafe { allocated.add(stack_size) };
        let return_value = unsafe { ReturnValuePtr::new(allocated.add(rv_offset) as *mut _) };

        Self {
            allocated,
            stack_top,
            return_value,
            layout,
        }
    }

    unsafe fn dealloc(self) {
        alloc::alloc::dealloc(self.allocated, self.layout)
    }
}

#[derive(Debug)]
#[must_use = "Dropping a JoinHandle leaks the associated thread and its resources"]
pub struct JoinHandle<T> {
    handle: OwnedHandle,
    memory: ThreadMemory<T>,
}

impl<T> JoinHandle<T>
where
    T: Send + 'static,
{
    pub fn join(self) -> Result<Option<T>> {
        let Self { handle, memory } = self;
        svc::wait_synchronization(handle.as_handle(), Timeout::forever())?;

        // SAFETY: The thread using this memory exited.
        // We own the only pointer to the location of the return value.
        // If the thread did not set a return value (because it panicked, for example), then this will be None.
        let return_value = unsafe { (*memory.return_value.0).read() };

        // SAFETY: The thread using this memory exited, so we have exclusive access and are free to
        // deallocate it.
        unsafe { memory.dealloc() };

        Ok(return_value)
    }

    pub fn is_running(&self) -> bool {
        svc::wait_synchronization(self.handle.as_handle(), Timeout::none()).is_err()
    }
}

#[derive(Debug)]
pub struct ThreadBuilder {
    priority: i32,
    stack_size: usize,
    processor_id: i32,
}

const fn align_to(value: usize, aligment: usize) -> usize {
    let mask = aligment - 1;
    (value + mask) & !mask
}

impl Default for ThreadBuilder {
    fn default() -> Self {
        Self {
            priority: 0x30,
            stack_size: 0x8000,
            processor_id: -2,
        }
    }
}

impl ThreadBuilder {
    pub fn with_priority(self, priority: i32) -> Self {
        Self { priority, ..self }
    }

    pub fn spawn<F, T>(self, f: F) -> Result<JoinHandle<T>>
    where
        F: FnOnce() -> T,
        F: Send + 'static,
        T: Send + 'static,
    {
        let thread_memory = ThreadMemory::allocate(self.stack_size);

        let return_value = thread_memory.return_value;

        let thread_main = move || unsafe {
            early_debug!("ThreadBuilder::spawn(): Entered main wrapper.");
            let rv: T = f();
            early_debug!("ThreadBuilder::spawn(): Got return value.");
            return_value.store(rv);
        };
        let packet = ThreadPacket::new(thread_main);
        let argument = ThreadPacket::into_argument(packet);

        debug!(
            "Launching thread: priority={}, argument={:p}, mem_start={:p}, stack_top={:p}, return_value={:p}, processor_id={}",
            self.priority, argument as *const (),
            thread_memory.allocated,
            thread_memory.stack_top,
            &thread_memory.return_value,
            self.processor_id,
        );

        let handle = unsafe {
            svc::create_thread(
                self.priority,
                _ctru_rt_thread_start,
                argument,
                thread_memory.stack_top,
                self.processor_id,
            )?
        };

        Ok(JoinHandle {
            handle,
            memory: thread_memory,
        })
    }
}

pub fn spawn<F, T>(f: F) -> Result<JoinHandle<T>>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    ThreadBuilder::default().spawn(f)
}
