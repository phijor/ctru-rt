// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#[derive(Debug, Clone, Copy)]
pub struct CfgMemPointer<T> {
    addr: usize,
    _size: PhantomData<T>,
}

impl<T> CfgMemPointer<T> {
    pub const unsafe fn new(addr: usize) -> Self {
        Self {
            addr,
            _size: PhantomData,
        }
    }

    pub fn read(&self) -> T {
        unsafe { ::core::ptr::read_volatile(self.addr as *const T) }
    }
}

use core::marker::PhantomData;

macro_rules! cfgmem_entry {
    ($addr: expr, $name: ident, $width: ty) => {
        pub const $name: CfgMemPointer<$width> = unsafe { CfgMemPointer::new($addr) };
    };
}

// cfgmem_entry!(0x1FF80000, KERNEL_?, u8);
cfgmem_entry!(0x1FF80001, KERNEL_VERSIONREVISION, u8);
cfgmem_entry!(0x1FF80002, KERNEL_VERSIONMINOR, u8);
cfgmem_entry!(0x1FF80003, KERNEL_VERSIONMAJOR, u8);
cfgmem_entry!(0x1FF80004, UPDATEFLAG, u32);
cfgmem_entry!(0x1FF80008, NSTID, u64);
cfgmem_entry!(0x1FF80010, SYSCOREVER, usize);
cfgmem_entry!(0x1FF80014, ENVINFO, u8);
cfgmem_entry!(0x1FF80015, UNITINFO, u8);
cfgmem_entry!(0x1FF80016, PREV_FIRM, u8);
cfgmem_entry!(0x1FF80018, KERNEL_CTRSDKVERSION, u32);
cfgmem_entry!(0x1FF80030, APPMEMTYPE, usize);
cfgmem_entry!(0x1FF80040, APPMEMALLOC, usize);
cfgmem_entry!(0x1FF80044, SYSMEMALLOC, usize);
cfgmem_entry!(0x1FF80048, BASEMEMALLOC, usize);
// cfgmem_entry!(0x1FF80060, FIRM_?, u8);
cfgmem_entry!(0x1FF80061, FIRM_VERSIONREVISION, u8);
cfgmem_entry!(0x1FF80062, FIRM_VERSIONMINOR, u8);
cfgmem_entry!(0x1FF80063, FIRM_VERSIONMAJOR, u8);
cfgmem_entry!(0x1FF80064, FIRM_SYSCOREVER, usize);
cfgmem_entry!(0x1FF80068, FIRM_CTRSDKVERSION, usize);
