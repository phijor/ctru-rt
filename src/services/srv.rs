use crate::{
    ipc::{IpcRequest, ThisProcessId},
    os::Handle,
    result::Result,
    svc,
};

use ctru_rt_macros::EnumCast;

#[derive(Debug, Copy, Clone, EnumCast)]
#[enum_cast(value_type = "u32")]
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
        IpcRequest::command(0x1)
            .translate_parameter(ThisProcessId)
            .dispatch(self.handle.handle())
            .map(drop)
    }

    pub fn enable_notifications(&self) -> Result<Handle> {
        let reply = IpcRequest::command(0x2).dispatch(self.handle.handle())?;
        Ok(unsafe { reply.finish_results().read_handle() })
    }

    pub fn get_service_handle(&self, service_name: &str) -> Result<Handle> {
        let ((arg0, arg1), len) = unsafe { write_str_param(service_name) };

        let mut reply = IpcRequest::command(0x5)
            .parameters(&[arg0, arg1, len, self.blocking_policy.to_value()])
            .dispatch(self.handle.handle())?
            .finish_results();

        Ok(unsafe { reply.read_handle() })
    }
}

unsafe fn write_str_param(s: &str) -> ((u32, u32), u32) {
    union Buf {
        words: [u32; 2],
        bytes: [u8; 2 * 4],
    }

    let mut buf: Buf = Buf { words: [0; 2] };

    let s_bytes = s.as_bytes();
    let n = buf.bytes.len().min(s_bytes.len());

    &buf.bytes[..n].copy_from_slice(&s_bytes[..n]);
    ((buf.words[0], buf.words[1]), n as u32)
}
