use crate::{
    ipc::IpcRequest,
    os::{
        mem::MemoryPermission,
        sharedmem::{MappedBlock, SharedMemoryMapper},
        Handle, SystemTick,
    },
    ports::srv::Srv,
    result::Result,
};

use log::debug;

use core::fmt;
use core::mem::ManuallyDrop;

#[derive(Debug)]
struct SharedMemory {
    sharedmem: ManuallyDrop<MappedBlock>,
}

impl SharedMemory {
    fn new(memory_handle: Handle) -> Result<Self> {
        const SIZE: usize = 0x2b0;
        const SELF_PERM: MemoryPermission = MemoryPermission::R;
        const HID_PERM: MemoryPermission = MemoryPermission::DontCare;
        let sharedmem =
            SharedMemoryMapper::global().map(memory_handle, SIZE, SELF_PERM, HID_PERM)?;

        let sharedmem = ManuallyDrop::new(sharedmem);

        Ok(Self { sharedmem })
    }

    unsafe fn read<T>(&self, offset: isize) -> T {
        (self.sharedmem.as_ptr().offset(offset) as *const T).read_volatile()
    }

    fn current_update(&self) -> SystemTick {
        let tick_count = unsafe { self.read(0) };

        SystemTick::new(tick_count)
    }

    fn last_update(&self) -> SystemTick {
        let tick_count = unsafe { self.read(2) };

        SystemTick::new(tick_count)
    }

    fn current_index(&self) -> u32 {
        let idx: u32 = unsafe { self.read(4) };
        idx & 0b0111
    }

    fn current_pad_state(&self) -> u32 {
        unsafe { self.read(5) }
    }

    unsafe fn pad_state(&self, index: u32) -> *const u32 {
        debug_assert!(index < 8);
        self.sharedmem.as_ptr().offset(10 + (index * 4) as isize)
    }

    fn pad_current(&self, index: u32) -> u32 {
        unsafe { self.pad_state(index).offset(0).read_volatile() }
    }

    fn pad_pressed(&self, index: u32) -> u32 {
        unsafe { self.pad_state(index).offset(1).read_volatile() }
    }

    fn pad_released(&self, index: u32) -> u32 {
        unsafe { self.pad_state(index).offset(2).read_volatile() }
    }
}

impl Drop for SharedMemory {
    fn drop(&mut self) {
        let sharedmem = unsafe { ManuallyDrop::take(&mut self.sharedmem) };

        let _mem_handle = SharedMemoryMapper::global().unmap(sharedmem).ok();
    }
}

#[derive(Debug)]
pub struct Hid {
    service_handle: Handle,
    sharedmem: SharedMemory,
    pads: (Handle, Handle),
    accelerometer: Handle,
    gyroscope: Handle,
    debugpad: Handle,
}

impl Hid {
    pub fn init(srv: &Srv) -> Result<Self> {
        let service_handle = srv
            .get_service_handle("hid:USER")
            .or_else(|_| srv.get_service_handle("hid:SPVR"))?;

        // Get IPC handles, map memory
        debug!("Acquiring IPC handles for HID module...");
        let reply = IpcRequest::command(0xa).dispatch(service_handle.handle())?;

        let [memory_handle, pad0, pad1, accelerometer, gyroscope, debugpad]: [Handle; 6] =
            unsafe { reply.finish_results().read_translate_result() };

        debug!("Mapping HID shared memory...");
        // It's important to map memory last: if this fails, all handles above are dropped properly
        let sharedmem = SharedMemory::new(memory_handle)?;

        debug!("HID initialized!");
        Ok(Self {
            service_handle,
            sharedmem,
            pads: (pad0, pad1),
            accelerometer,
            gyroscope,
            debugpad,
        })
    }

    fn enable_accelerometer(&self) -> Result<()> {
        IpcRequest::command(0xa)
            .dispatch(self.service_handle.handle())
            .map(drop)
    }

    pub fn last_keypad(&self) -> KeyPad {
        debug!("tick (low): {:?}", self.sharedmem.current_update());
        let index = self.sharedmem.current_index();

        let pad = self.sharedmem.pad_current(index);

        KeyPad::new(pad)
    }
}

#[derive(Clone, Copy)]
pub struct KeyPad(u32);

#[doc(hidden)]
macro_rules! _keypad_key {
    ($name: ident, $index: expr) => {
        #[inline]
        pub const fn $name(&self) -> bool {
            self.0 & (1 << $index) != 0
        }
    };
}

impl KeyPad {
    pub const fn new(bits: u32) -> Self {
        Self(bits)
    }

    _keypad_key! {a, 0}
    _keypad_key! {b, 1}
    _keypad_key! {select, 2}
    _keypad_key! {start, 3}
    _keypad_key! {right, 4}
    _keypad_key! {left, 5}
    _keypad_key! {up, 6}
    _keypad_key! {down, 7}
    _keypad_key! {r, 8}
    _keypad_key! {l, 9}
    _keypad_key! {x, 10}
    _keypad_key! {y, 11}
    _keypad_key! {cpad_right, 28}
    _keypad_key! {cpad_left, 29}
    _keypad_key! {cpad_up, 30}
    _keypad_key! {cpad_down, 31}
}

struct DebugLiteral(&'static str);

impl fmt::Debug for DebugLiteral {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl fmt::Debug for KeyPad {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut keys = f.debug_set();

        let mut fmt_key = |name: &'static str, pressed: bool| {
            if pressed {
                keys.entry(&DebugLiteral(name));
            }
        };

        fmt_key("A", self.a());
        fmt_key("B", self.b());
        fmt_key("SELECT", self.select());
        fmt_key("START", self.start());
        fmt_key("RIGHT", self.right());
        fmt_key("LEFT", self.left());
        fmt_key("UP", self.up());
        fmt_key("DOWN", self.down());
        fmt_key("R", self.r());
        fmt_key("L", self.l());
        fmt_key("X", self.x());
        fmt_key("Y", self.y());
        fmt_key("CPAD_RIGHT", self.cpad_right());
        fmt_key("CPAD_LEFT", self.cpad_left());
        fmt_key("CPAD_UP", self.cpad_up());
        fmt_key("CPAD_DOWN", self.cpad_down());

        keys.finish()
    }
}
