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

    /// Register this process as a client of `srv:`
    fn register_client(&self) -> Result<()> {
        ipc::IpcRequest::command(0x1)
            .with_translate_params(&[TranslateParameterSet::ProcessId])
            .dispatch(&self.handle)
            .map(drop)
    }

    pub fn blocking_policy(&self) -> BlockingPolicy {
        self.blocking_policy
    }

    pub fn set_blocking_policy(&mut self, blocking_policy: BlockingPolicy) {
        self.blocking_policy = blocking_policy
    }

    pub fn enable_notifications(&self) -> Result<Handle> {
        let reply = ipc::IpcRequest::command(0x2).dispatch(&self.handle)?;
        Ok(reply
            .translate_values
            .expect("enable_notifications did not yield a handle")
            .get(0)
            .unwrap()
            .copy_raw())
    }

    pub fn get_service_handle(&self, service_name: &str) -> Result<Handle> {
        let (len, buf) = unsafe {
            let mut buf: [u32; 2] = [0; 2];
            (write_str_param(&mut buf, service_name) as u32, buf)
        };

        let reply = ipc::IpcRequest::command(0x5)
            .with_params(&[buf[0], buf[1], len, self.blocking_policy as u32])
            .dispatch(&self.handle)?;

        Ok(reply
            .translate_values
            .expect("get_service_handle did not yield a handle")
            .get(0)
            .unwrap()
            .copy_raw())
    }
}

impl Drop for Srv {
    fn drop(&mut self) {
        let handle = core::mem::replace(&mut self.handle, Handle::invalid());
        let _ = svc::close_handle(handle);
    }
}

unsafe fn write_str_param(buf: &mut [u32], s: &str) -> usize {
    let byte_buf = core::slice::from_raw_parts_mut(
        buf.as_mut_ptr() as *mut u8,
        buf.len() * core::mem::size_of::<u32>(),
    );

    let s_bytes = s.as_bytes();

    let n = byte_buf.len().min(s_bytes.len());
    &byte_buf[..n].copy_from_slice(&s_bytes[..n]);
    n
}
