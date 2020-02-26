pub struct ThreadLocalStorage(*mut u8);

impl ThreadLocalStorage {
    #[inline]
    pub fn command_buffer(&self) -> *mut u32 {
        unsafe { self.0.offset(0x80) as *mut u32 }
    }
}

#[inline]
pub fn get_thread_local_storage() -> ThreadLocalStorage {
    let data: *mut u8;
    unsafe { asm!("mrc p15, 0, $0, c13, c0, 3" : "=r"(data) : : :) }
    ThreadLocalStorage(data)
}
