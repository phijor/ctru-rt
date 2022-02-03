// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::marker::PhantomData;

/// TODO: figure out how to trick this code into giving me thread local access
struct AccessToken;

static mut THREAD_ACCESS_TOKEN: AccessToken = AccessToken;

pub struct ThreadLocalStorage(*mut u8);

impl ThreadLocalStorage {
    #[inline]
    pub fn command_buffer(&self) -> *mut u32 {
        unsafe { self.0.add(0x80) as *mut u32 }
    }

    #[inline]
    pub fn static_buffer_descriptors(&self) -> StaticBufferDescriptors {
        unsafe { StaticBufferDescriptors::new(self.0.add(0x180) as *mut u32) }
    }
}

#[inline]
pub fn get_thread_local_storage() -> ThreadLocalStorage {
    let data: *mut u8;
    unsafe {
        asm!(
            "mrc p15, 0, {0}, c13, c0, 3",
            out(reg) data,
        )
    }
    ThreadLocalStorage(data)
}

#[repr(packed)]
struct Descriptor<'a> {
    flags: u32,
    ptr: *mut (),
    _lifetime: PhantomData<&'a [u8]>,
}

pub struct StaticBufferDescriptors<'a> {
    descriptors: *mut Descriptor<'a>,
}

impl<'a> StaticBufferDescriptors<'a> {
    unsafe fn new(ptr: *mut u32) -> Self {
        Self {
            descriptors: ptr as *mut Descriptor<'a>,
        }
    }

    pub fn set<T: 'a>(&mut self, index: usize, data: &'a mut [T]) {
        let index = index & 0b1111;
        let flags = (1 << 1)
            | ((data.len() * core::mem::size_of::<T>()) << 14) as u32
            | (index << 10) as u32;

        unsafe {
            self.descriptors.add(index).write(Descriptor {
                flags,
                ptr: data.as_ptr() as *mut (),
                _lifetime: PhantomData,
            })
        }
    }
}
