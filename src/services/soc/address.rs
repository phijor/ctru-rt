// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::AddressFamily;

use core::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

#[repr(C, packed)]
pub(super) struct RawAddr {
    len: u8,
    family: u8,
    inner: RawAddrInner,
}

#[repr(C, packed)]
union RawAddrInner {
    unspec: RawAddrUnspec,
    v6: RawAddr6,
    v4: RawAddr4,
}

#[derive(Copy, Clone)]
#[repr(C)]
struct RawAddrUnspec([u8; 0x1c - 2]);

#[derive(Copy, Clone)]
#[repr(C, packed)]
struct RawAddr4 {
    port: u16,
    addr: [u8; 4],
}

#[derive(Copy, Clone)]
#[repr(C, packed)]
struct RawAddr6 {
    port: u16,
    addr: [u8; 16],
    flowinfo: u32,
    scope_id: u32,
}

const _: () = assert!(size_of::<RawAddr>() == 0x1c);

impl RawAddr {
    pub(super) const fn unspec() -> Self {
        Self {
            len: 0,
            family: AddressFamily::AF_UNSPEC as u8,
            inner: RawAddrInner {
                unspec: RawAddrUnspec([0; _]),
            },
        }
    }

    pub(super) fn as_out_bytes(&mut self) -> &mut [u8] {
        let data = core::ptr::from_mut(self).cast::<u8>();
        unsafe { core::slice::from_raw_parts_mut(data, size_of::<Self>()) }
    }

    pub(super) fn as_in_bytes(&self) -> &[u8] {
        let len = self.len as usize;
        assert!(len <= size_of::<Self>());
        let data = core::ptr::from_ref(self).cast::<u8>();
        unsafe { core::slice::from_raw_parts(data, len) }
    }

    fn family(&self) -> AddressFamily {
        self.family.into()
    }

    pub(super) fn decode(&self) -> Option<SocketAddr> {
        match self.family() {
            AddressFamily::AF_INET if self.len >= 8 => {
                let RawAddr4 { port, addr } = unsafe { self.inner.v4 };
                let port = u16::from_be(port);
                let addr = Ipv4Addr::from_octets(addr);
                Some(SocketAddr::V4(SocketAddrV4::new(addr, port)))
            }
            AddressFamily::AF_INET6 if self.len >= 0x1c => {
                let RawAddr6 {
                    port,
                    addr,
                    flowinfo,
                    scope_id,
                }: RawAddr6 = unsafe { self.inner.v6 };
                Some(SocketAddr::V6(SocketAddrV6::new(
                    Ipv6Addr::from_octets(addr),
                    u16::from_be(port),
                    u32::from_be(flowinfo),
                    u32::from_be(scope_id),
                )))
            }
            _ => None,
        }
    }

    pub(super) fn encode(addr: &SocketAddr) -> Self {
        // Start from a valid raw address whose bytes are all initialized, then assign either the
        // union field v4 or v6, depending on the given `SocketAddr`. This ensures as_{in,out}_bytes
        // do not hand out uninitialized memory.
        let mut raw = Self::unspec();
        match addr {
            SocketAddr::V4(addr_v4) => {
                raw.len = 8;
                raw.family = AddressFamily::AF_INET.into();
                raw.inner.v4 = RawAddr4 {
                    port: u16::to_be(addr_v4.port()),
                    addr: addr_v4.ip().octets(),
                };
            }
            SocketAddr::V6(addr_v6) => {
                raw.len = 0x1c;
                raw.family = AddressFamily::AF_INET6.into();
                raw.inner.v6 = RawAddr6 {
                    port: u16::to_be(addr_v6.port()),
                    addr: addr_v6.ip().octets(),
                    flowinfo: addr_v6.flowinfo().to_be(),
                    scope_id: addr_v6.scope_id().to_be(),
                };
            }
        }
        raw
    }
}
