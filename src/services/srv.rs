use crate::{
    ipc::{self, TranslateParameterSet},
    os::Handle,
    result::Result,
    svc,
};

#[derive(Debug, Copy, Clone)]
pub enum BlockingPolicy {
    Blocking = 0,
    NonBlocking = 1,
}

#[derive(Debug)]
pub struct Srv {
    handle: Handle,
    blocking_policy: BlockingPolicy,
}

impl Srv {
    pub fn init() -> Result<Self> {
        let srv = Self {
            handle: svc::connect_to_port("srv:\0")?,
            blocking_policy: BlockingPolicy::Blocking,
        };

        srv.register_client()?;

        Ok(srv)
    }

    pub fn blocking_policy(&self) -> BlockingPolicy {
        self.blocking_policy
    }

    pub fn set_blocking_policy(&mut self, blocking_policy: BlockingPolicy) {
        self.blocking_policy = blocking_policy
    }

    /// Register this process as a client of `srv:`
    fn register_client(&self) -> Result<()> {
        ipc::IpcRequest::command(0x1)
            .with_translate_params(&[TranslateParameterSet::ProcessId])
            .dispatch(self.handle.handle())
            .map(drop)
    }

    pub fn enable_notifications(&self) -> Result<Handle> {
        let reply = ipc::IpcRequest::command(0x2).dispatch(self.handle.handle())?;
        Ok(unsafe { Handle::own(reply.translate_values[0]) })
    }

    pub fn get_service_handle(&self, service_name: &str) -> Result<Handle> {
        let ((arg0, arg1), len) = unsafe { write_str_param(service_name) };

        let reply = ipc::IpcRequest::command(0x5)
            .with_params(&[arg0, arg1, len, self.blocking_policy as u32])
            .dispatch(self.handle.handle())?;

        Ok(unsafe { Handle::own(reply.translate_values[0]) })
    }
}

unsafe fn write_str_param(s: &str) -> ((u32, u32), u32) {
    let mut buf: [u32; 2] = [0; 2];
    let byte_buf = core::slice::from_raw_parts_mut(
        buf.as_mut_ptr() as *mut u8,
        buf.len() * core::mem::size_of::<u32>(),
    );

    let s_bytes = s.as_bytes();

    let n = byte_buf.len().min(s_bytes.len());
    &byte_buf[..n].copy_from_slice(&s_bytes[..n]);
    ((buf[0], buf[1]), n as u32)
}
