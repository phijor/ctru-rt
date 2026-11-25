// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::address::RawAddr;

const CANON_NAME_LEN: usize = 256;

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

const _RAW_ADDR_INFO_SIZE: () = assert!(size_of::<RawAddrInfo>() == 0x130);

impl RawAddrInfo {
    pub const fn as_bytes_mut(addrs: &mut [Self]) -> &mut [u8] {
        let len = size_of_val(addrs);
        let data = core::ptr::from_mut(addrs).cast::<u8>();

        // SAFETY:
        //
        // `RawAddrInfo` has no internal or trailing padding
        // (otherwise `_RAW_ADDR_INFO_SIZE` would not hold),
        // so all bytes in the slice are valid to read.
        //
        // All bit patterns are valid values of `RawAddrInfo`:
        // its fields are integers, a byte array and `RawAddr`,
        // all of which accept arbitrary bit patterns.
        // Thus, writing through the returned byte slice is safe.
        unsafe { core::slice::from_raw_parts_mut(data, len) }
    }
}

type Nullptr = u32;

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

const _RAW_HINTS_SIZE: () = assert!(size_of::<RawHints>() == 0x20);

impl RawHints {
    pub const fn as_bytes(hints: Option<&Self>) -> &[u8] {
        let hints: &[Self] = match hints {
            Some(hint) => core::slice::from_ref(hint),
            None => &[],
        };

        let len = size_of_val(hints);
        let data = core::ptr::from_ref(hints).cast::<u8>();

        // SAFETY: RawHints has no internal or trailing padding
        // (otherwise `_RAW_HINTS_SIZE` would not hold),
        // so all bytes in the slice are valid to read.
        unsafe { core::slice::from_raw_parts(data, len) }
    }
}
