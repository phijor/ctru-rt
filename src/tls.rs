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
    unsafe fn static_buffer_descriptors<const N: usize>(&self) -> *mut [Descriptor; N] {
        unsafe { self.0.add(0x180) as *mut [Descriptor; N] }
    }

    #[inline]
    pub(crate) fn with_receive_buffers<const N: usize, R>(
        &mut self,
        buffers: &mut [&mut [u8]; N],
        f: impl FnOnce() -> R,
    ) -> R {
        let descriptors = unsafe { StaticBufferDescriptors::set(self, buffers) };
        let result = f();
        drop(descriptors);
        result
    }
}

#[inline]
pub fn get_thread_local_storage() -> ThreadLocalStorage {
    let data: *mut u8;
    unsafe {
        core::arch::asm!(
            "mrc p15, 0, {0}, c13, c0, 3",
            options(pure, nomem, nostack, preserves_flags),
            out(reg) data,
        )
    }
    ThreadLocalStorage(data)
}

#[repr(C)]
struct Descriptor {
    flags: u32,
    ptr: *mut u8,
}

impl Descriptor {
    #[inline]
    fn from_slice(data: &mut [u8]) -> Self {
        let size = core::mem::size_of_val(data);

        assert!(size <= 0x3FFFF, "Static buffer size too large");

        let flags = (1 << 1) | (size << 14) as u32;
        let ptr = data.as_mut_ptr();
        Self { flags, ptr }
    }

    #[inline]
    const fn invalid() -> Self {
        Self {
            flags: 0,
            ptr: core::ptr::null_mut(),
        }
    }
}

struct StaticBufferDescriptors<'tls, 'buf, const N: usize> {
    descriptors: *mut [Descriptor; N],
    _tls: PhantomData<&'tls mut ThreadLocalStorage>,
    _buf: PhantomData<&'buf mut [u8]>,
}

impl<'tls, 'buf, const N: usize> StaticBufferDescriptors<'tls, 'buf, N> {
    /// Set the first `N` descriptors to point to some buffers.
    ///
    /// # Safety
    ///
    /// The returned guard must not be leaked: its destructor ensures that the pointers to `buffers` are removed from
    /// the buffer descriptors, and replaced by null pointers.
    unsafe fn set(tls: &'tls mut ThreadLocalStorage, buffers: &'buf mut [&mut [u8]; N]) -> Self {
        const { assert!(N <= 16, "Cannot set more than 16 receive buffers") };

        let descriptors = unsafe { tls.static_buffer_descriptors() };

        let mut descriptor = descriptors as *mut Descriptor;
        for buffer in buffers.iter_mut() {
            unsafe {
                descriptor.write(Descriptor::from_slice(buffer));
                descriptor = descriptor.add(1);
            };
        }

        Self {
            descriptors,
            _tls: PhantomData,
            _buf: PhantomData,
        }
    }
}

impl<const N: usize> Drop for StaticBufferDescriptors<'_, '_, N> {
    fn drop(&mut self) {
        // Set the first `N` descriptors to point to [`core::ptr::null_mut`].
        let mut descriptor = self.descriptors as *mut Descriptor;
        for _ in 0..N {
            unsafe {
                descriptor.write(Descriptor::invalid());
                descriptor = descriptor.add(1);
            };
        }
    }
}
