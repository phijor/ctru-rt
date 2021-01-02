use crate::{
    ipc::IpcRequest,
    os::{
        sharedmem::{MappedBlock, SharedMemoryMapper},
        Handle, WeakHandle,
    },
    result::Result,
    services::srv::Srv,
    svc,
};

use log::debug;

use core::mem::ManuallyDrop;

#[derive(Debug)]
pub struct Hid<'m> {
    service_handle: Handle,
    sharedmem: ManuallyDrop<MappedBlock>,
    memory_mapper: &'m mut SharedMemoryMapper,
    pads: (Handle, Handle),
    accelerometer: Handle,
    gyroscope: Handle,
    debugpad: Handle,
}

impl<'m> Hid<'m> {
    pub fn init<'s>(srv: &'s Srv, memory_mapper: &'m mut SharedMemoryMapper) -> Result<Self> {
        let service_handle = srv
            .get_service_handle("hid:USER")
            .or_else(|_| srv.get_service_handle("hid:SPVR"))?;

        // Get IPC handles, map memory
        debug!("Acquiring IPC handles for HID module...");
        let reply = IpcRequest::command(0xa).dispatch(service_handle.handle())?;

        unsafe {
            let pads = (
                Handle::own(reply.translate_values[1]),
                Handle::own(reply.translate_values[2]),
            );
            let accelerometer = Handle::own(reply.translate_values[3]);
            let gyroscope = Handle::own(reply.translate_values[4]);
            let debugpad = Handle::own(reply.translate_values[5]);

            debug!("Mapping HID shared memory...");
            // It's important to map memory last: if this fails, all handles are dropped properly
            let memory_handle = Handle::own(reply.translate_values[0]);
            let sharedmem = ManuallyDrop::new(memory_mapper.map(memory_handle, 0x2b0)?);

            debug!("HID initialized!");
            Ok(Self {
                service_handle,
                memory_mapper,
                sharedmem,
                pads,
                accelerometer,
                gyroscope,
                debugpad,
            })
        }
    }

    fn enable_accelerometer(&self) -> Result<()> {
        IpcRequest::command(0xa)
            .dispatch(self.service_handle.handle())
            .map(drop)
    }

    pub fn last_keypad(&self) -> u32 {
        let sharedmem = self.sharedmem.as_slice().as_ptr();

        unsafe {
            debug!("tick (low): {}", sharedmem.read());
            let last_updated = sharedmem.offset(4).read_volatile().max(7) as isize;

            sharedmem.offset(10 + last_updated * 4).read_volatile()
        }
    }
}

impl<'m> Drop for Hid<'m> {
    fn drop(&mut self) {
        // TODO: this is certainly not correct.
        unsafe {
            let _ = self
                .memory_mapper
                .unmap(ManuallyDrop::take(&mut self.sharedmem))
                .map(drop);
        };
    }
}
