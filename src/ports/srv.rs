// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::{
    ipc::{IpcRequest, ThisProcessId},
    os::{AsHandle, OwnedHandle},
    result::Result,
    svc,
};

use ctru_rt_macros::EnumCast;
use log::debug;

#[derive(Debug, Copy, Clone, EnumCast)]
#[enum_cast(value_type = "u32")]
pub enum BlockingPolicy {
    Blocking = 0,
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
        let ((name0, name1), len) = unsafe { write_str_param(service_name) };
        let mut reply = IpcRequest::command(0x3)
            .parameters(&[name0, name1, len, max_sessions])
            .dispatch(&self.handle)?
            .finish_results();

        Ok(unsafe { reply.read_handle() })
    }

    pub fn unregister_service(&self, service_name: &str) -> Result<()> {
        let ((name0, name1), len) = unsafe { write_str_param(service_name) };
        let _reply = IpcRequest::command(0x4)
            .parameters(&[name0, name1, len])
            .dispatch(self.as_handle())?;

        Ok(())
    }

    pub fn get_service_handle(&self, service_name: &str) -> Result<OwnedHandle> {
        let ((arg0, arg1), len) = unsafe { write_str_param(service_name) };

        let mut reply = IpcRequest::command(0x5)
            .parameters(&[arg0, arg1, len, self.blocking_policy.to_value()])
            .dispatch(self.as_handle())?
            .finish_results();

        Ok(unsafe { reply.read_handle() })
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
        let ((arg0, arg1), len) = unsafe { write_str_param(service_name) };
        let mut reply = IpcRequest::command(0xe)
            .parameters(&[arg0, arg1, len])
            .dispatch(self.as_handle())?;

        Ok(reply.read_word() != 0)
    }
}

fn publish_flags(coalesc_pending: bool, ignore_overflow: bool) -> u32 {
    u32::from(coalesc_pending) | u32::from(ignore_overflow) << 1
}

unsafe fn write_str_param(s: &str) -> ((u32, u32), u32) {
    union Buf {
        words: [u32; 2],
        bytes: [u8; 2 * 4],
    }

    let mut buf: Buf = Buf { words: [0; 2] };

    let s_bytes = s.as_bytes();
    let n = buf.bytes.len().min(s_bytes.len());

    buf.bytes[..n].copy_from_slice(&s_bytes[..n]);
    ((buf.words[0], buf.words[1]), n as u32)
}

impl AsHandle for Srv {
    fn as_handle(&self) -> crate::os::BorrowedHandle {
        self.handle.as_handle()
    }
}
