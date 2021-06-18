use crate::early_debug;
use crate::os::{BorrowHandle, Handle};
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

    svc::exit_thread();
}

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
pub struct Thread {
    handle: Handle,
    memory_start: *mut u8,
    memory_layout: Layout,
}

impl Thread {
    pub fn join(self) -> Result<()> {
        let Self {
            handle,
            memory_start,
            memory_layout,
        } = self;
        svc::wait_synchronization(handle.borrow_handle(), Timeout::forever())?;

        // SAFETY: The thread using this memory exited, so we have exclusive access and are free to
        // deallocate it.
        unsafe { alloc::alloc::dealloc(memory_start, memory_layout) };

        Ok(())
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
            stack_size: 0x1000,
            processor_id: -2,
        }
    }
}

impl ThreadBuilder {
    pub fn with_priority(self, priority: i32) -> Self {
        Self { priority, ..self }
    }

    fn allocate_thread_memory(&self) -> (*mut u8, Layout) {
        let layout = Layout::array::<u8>(align_to(self.stack_size, 8))
            .unwrap()
            .align_to(8)
            .unwrap();

        let ptr = unsafe { alloc::alloc::alloc(layout) };
        (ptr, layout)
    }

    pub fn spawn<F>(self, f: F) -> Result<Thread>
    where
        F: FnOnce(),
        F: Send + 'static,
    {
        let (memory_start, memory_layout) = self.allocate_thread_memory();

        let stack_top = unsafe { memory_start.add(memory_layout.size()) };

        let packet = ThreadPacket::new(f);
        let argument = ThreadPacket::into_argument(packet);

        debug!(
            "Launching thread: priority={}, argument={:p}, stack_top={:p}, processor_id={}",
            self.priority, argument as *const (), stack_top, self.processor_id
        );

        let handle = unsafe {
            svc::create_thread(
                self.priority,
                _ctru_rt_thread_start,
                argument,
                stack_top,
                self.processor_id,
            )?
        };

        Ok(Thread {
            handle,
            memory_start,
            memory_layout,
        })
    }
}

pub fn spawn<F: FnOnce() + Send + Sync + 'static>(f: F) -> Result<Thread> {
    ThreadBuilder::default().spawn(f)
}
