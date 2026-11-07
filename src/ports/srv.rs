// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::{
    ipc::{CommandBufferWriter, IpcRequest, Parameter, ThisProcessId},
    os::{AsHandle, OwnedHandle},
    result::{ERROR_NOT_AUTHORIZED, Result},
    svc,
};

use log::debug;
use num_enum::{FromPrimitive, IntoPrimitive};

#[derive(Debug, Copy, Clone, FromPrimitive, IntoPrimitive)]
#[repr(u32)]
pub enum BlockingPolicy {
    Blocking = 0,
    #[num_enum(default)]
    NonBlocking = 1,
}

#[derive(Debug)]
pub struct Srv {
    handle: OwnedHandle,
    blocking_policy: BlockingPolicy,
}

impl Srv {
    pub fn init() -> Result<Self> {
        debug!("Connecting to port `srv:`...");
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
        debug!("Registering this process as client of `srv:`...");
        IpcRequest::command(0x1)
            .translate_parameter(ThisProcessId)
            .dispatch(&self.handle)
            .map(drop)
    }

    pub fn enable_notifications(&self) -> Result<OwnedHandle> {
        let reply = IpcRequest::command(0x2).dispatch(&self.handle)?;
        Ok(unsafe { reply.finish_results().read_handle() })
    }

    pub fn register_service(&self, service_name: &str, max_sessions: u32) -> Result<OwnedHandle> {
        let mut reply = IpcRequest::command(0x3)
            .parameter(StringParam::new(service_name))
            .parameter(max_sessions)
            .dispatch(&self.handle)?
            .finish_results();

        Ok(unsafe { reply.read_handle() })
    }

    pub fn unregister_service(&self, service_name: &str) -> Result<()> {
        let _reply = IpcRequest::command(0x4)
            .parameter(StringParam::new(service_name))
            .dispatch(self.as_handle())?;

        Ok(())
    }

    pub fn get_service_handle(&self, service_name: &str) -> Result<OwnedHandle> {
        let mut reply = IpcRequest::command(0x5)
            .parameter(StringParam::new(service_name))
            .parameter(u32::from(self.blocking_policy))
            .dispatch(self.as_handle())?
            .finish_results();

        Ok(unsafe { reply.read_handle() })
    }

    pub fn get_service_handle_alternatives(
        &self,
        service_names: &[&str],
    ) -> Result<(OwnedHandle, usize)> {
        let mut result = ERROR_NOT_AUTHORIZED;
        for (service_name, i) in service_names.iter().zip(0..) {
            match self.get_service_handle(service_name) {
                Ok(handle) => {
                    return Ok((handle, i));
                }
                Err(err) => {
                    result = err;
                }
            }
        }

        Err(result)
    }

    pub fn subscribe(&self, notification_id: u32) -> Result<()> {
        let _reply = IpcRequest::command(0x9)
            .parameter(notification_id)
            .dispatch(self.as_handle())?;

        Ok(())
    }

    pub fn unsubscribe(&self, notification_id: u32) -> Result<()> {
        let _reply = IpcRequest::command(0xa)
            .parameter(notification_id)
            .dispatch(self.as_handle())?;

        Ok(())
    }

    pub fn receive_notification(&self) -> Result<u32> {
        let mut reply = IpcRequest::command(0xb).dispatch(self.as_handle())?;

        Ok(reply.read_word())
    }

    pub fn publish_notification(
        &self,
        notification_id: u32,
        coalesc_pending: bool,
        ignore_overflow: bool,
    ) -> Result<()> {
        let _reply = IpcRequest::command(0xc)
            .parameters(&[
                notification_id,
                publish_flags(coalesc_pending, ignore_overflow),
            ])
            .dispatch(self.as_handle())?;

        Ok(())
    }

    pub fn publish_notification_get_subscribers<'s>(
        &self,
        notification_id: u32,
        coalesc_pending: bool,
        ignore_overflow: bool,
        subscribers: &'s mut [u32],
    ) -> Result<&'s [u32]> {
        let mut reply = IpcRequest::command(0xd)
            .parameters(&[
                notification_id,
                publish_flags(coalesc_pending, ignore_overflow),
            ])
            .dispatch(self.as_handle())?;

        let num_subscribers = reply.read_word() as usize;

        let subscribers = &mut subscribers[..num_subscribers];
        for subscriber in subscribers.iter_mut() {
            *subscriber = reply.read_word();
        }

        Ok(subscribers)
    }

    pub fn is_service_registered(&self, service_name: &str) -> Result<bool> {
        let mut reply = IpcRequest::command(0xe)
            .parameter(StringParam::new(service_name))
            .dispatch(self.as_handle())?;

        Ok(reply.read_word() != 0)
    }
}

fn publish_flags(coalesc_pending: bool, ignore_overflow: bool) -> u32 {
    u32::from(coalesc_pending) | u32::from(ignore_overflow) << 1
}

struct StringParam {
    words: [u32; 2],
    len: usize,
}

impl StringParam {
    fn new(s: &str) -> StringParam {
        const BUF_WORDS: usize = 2;
        const BUF_BYTES: usize = BUF_WORDS * size_of::<u32>();

        union Buf {
            words: [u32; BUF_WORDS],
            bytes: [u8; BUF_BYTES],
        }

        let mut buf: Buf = Buf { words: [0; _] };

        let bytes = s.as_bytes();
        let len = bytes.len().min(BUF_BYTES);

        // Copy at most BUF_BYTES from the string
        unsafe { buf.bytes[..len].copy_from_slice(&bytes[..len]) };
        let words = unsafe { buf.words };

        Self { words, len }
    }
}

impl Parameter for StringParam {
    fn encode(&self, cmdbuf: &mut CommandBufferWriter) {
        self.words[0].encode(cmdbuf);
        self.words[1].encode(cmdbuf);
        self.len.encode(cmdbuf)
    }
}

impl AsHandle for Srv {
    fn as_handle(&self) -> crate::os::BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

pub struct LazyServiceSession {
    srv: Srv,
    service_names: &'static [&'static str],
}

impl LazyServiceSession {
    pub fn new(srv: Srv, service_names: &'static [&'static str]) -> Self {
        Self { srv, service_names }
    }

    pub fn open(&mut self) -> Result<OwnedHandle> {
        let (handle, matched_offset) = self
            .srv
            .get_service_handle_alternatives(self.service_names)?;
        self.service_names = &self.service_names[matched_offset..];

        Ok(handle)
    }
}
