// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::mem::MaybeUninit;

use super::address::RawAddr;
use super::{AddressFamily, Protocol, Type};

const CANON_NAME_LEN: usize = 0x100;

/// The raw address information written by [`getaddrinfo`](super::Soc::getaddrinfo).
///
/// # Invariants
///
/// * **I1:**
///   `RawAddrInfo` has no internal or trailing padding.
///   This is checked by `_RAW_ADDR_INFO_SIZE`.
/// * **I2:**
///   All bit patterns are valid values of `RawAddrInfo`:
///   its fields are integers, a byte array and `RawAddr`,
///   all of which accept arbitrary bit patterns.
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct RawAddrInfo {
    flags: u32,
    family: u32,
    socket_type: u32,
    protocol: u32,
    address_len: u32,
    canon_name: [u8; CANON_NAME_LEN],
    address: RawAddr,
}

/// I1: Ensure that `RawAddrInfo` has no internal or trailing padding.
const _RAW_ADDR_INFO_SIZE: () = assert!(size_of::<RawAddrInfo>() == 0x130);

impl RawAddrInfo {
    pub const fn zeroed() -> Self {
        // SAFETY: (I2) ⇒ All-zero bytes is a valid bit pattern.
        unsafe { MaybeUninit::zeroed().assume_init() }
    }

    pub fn address(&self) -> &RawAddr {
        &self.address
    }

    pub const fn as_bytes_mut(addrs: &mut [Self]) -> &mut [u8] {
        let len = size_of_val(addrs);
        let data = core::ptr::from_mut(addrs).cast::<u8>();

        // SAFETY: `data` is non-null and `[data, data+len)` is valid for both reads and writes:
        // * (I1) ⇒ All bytes in the slice are valid to read.
        // * (I2) ⇒ Writing through the slice is safe.
        unsafe { core::slice::from_raw_parts_mut(data, len) }
    }
}

type Nullptr = u32;

/// The raw hints used by [`getaddrinfo`](super::Soc::getaddrinfo) when looking up addresses.
///
/// # Invariants
///
/// * **I1:**
///   `RawHints` has no internal or trailing padding.
///   This is checked by `_RAW_HINTS_SIZE`.
/// * **I2:**
///   All bit patterns are valid values of `RawHints`:
///   its fields are integers which accept arbitrary bit patterns.
#[repr(C)]
pub(super) struct RawHints {
    flags: u32,
    family: u32,
    socket_type: u32,
    protocol: u32,
    _address_len: Nullptr,
    _canon_name: Nullptr,
    _address: Nullptr,
    _next: Nullptr,
}

/// I1: Ensure that `RawHints` has no internal or trailing padding.
const _RAW_HINTS_SIZE: () = assert!(size_of::<RawHints>() == 0x20);

impl RawHints {
    pub(super) fn new(
        flags: u32,
        family: AddressFamily,
        socket_type: Type,
        protocol: Protocol,
    ) -> Self {
        Self {
            flags,
            family: u32::from(u8::from(family)),
            socket_type: u32::from(socket_type),
            protocol: u32::from(protocol),
            _address_len: 0,
            _canon_name: 0,
            _address: 0,
            _next: 0,
        }
    }

    pub const fn as_bytes(hints: Option<&Self>) -> &[u8] {
        let hints: &[Self] = match hints {
            Some(hint) => core::slice::from_ref(hint),
            None => &[],
        };

        let len = size_of_val(hints);
        let data = core::ptr::from_ref(hints).cast::<u8>();

        // SAFETY: `data` is non-null and `[data, data+len)` is valid for reads:
        // * (I1) ⇒ All bytes in the slice are valid to read.
        unsafe { core::slice::from_raw_parts(data, len) }
    }
}
