use crate::result::ResultCode;

extern "C" {
    fn svcOutputDebugString(message: *const u8, length: usize);
    fn svcExitProcess();
    fn svcControlMemory(
        dest: *mut usize,
        addr0: usize,
        addr1: usize,
        size: usize,
        op: u32,
        permission: u32,
    ) -> ResultCode;
    fn svcGetSystemInfo(out: *mut i64, sysinfo_type: u32, param: i32);
}

pub fn output_debug_string(message: &str) {
    unsafe { svcOutputDebugString(message.as_ptr(), message.len()) }
}

pub fn exit_process() -> ! {
    unsafe { svcExitProcess() }

    loop {}
}

pub mod mem {
    use crate::os::MemoryRegion;

    #[repr(u32)]
    pub enum MemoryOperationTarget {
        Heap = 0x0_0000,
        Linear = 0x1_0000,
    }

    #[repr(u32)]
    pub enum MemoryOperationRegion {
        App = (MemoryRegion::Application as u32) << 16,
        System = (MemoryRegion::System as u32) << 16,
        Base = (MemoryRegion::Base as u32) << 16,
    }

    #[repr(u32)]
    pub enum MemoryOperationAction {
        Free = 1,
        Reserve = 2,
        Allocate = 3,
        Map = 4,
        Unmap = 5,
        ChangeProtection = 6,
    }

    pub struct MemoryOperation(pub(crate) u32);

    impl MemoryOperation {
        #[inline]
        pub const fn new(
            action: MemoryOperationAction,
            region: MemoryOperationRegion,
            target: MemoryOperationTarget,
        ) -> Self {
            Self((action as u32) | (region as u32) | (target as u32))
        }

        #[inline]
        pub const fn allocate() -> Self {
            Self(MemoryOperationAction::Allocate as u32)
        }
    }

    #[repr(u32)]
    pub enum MemoryPermission {
        None = 0,
        R = 1,
        W = 2,
        Rw = 3,
        X = 4,
        Rx = 5,
        Wx = 6,
        Rwx = 7,
        DontCare = 0x10000000,
    }
}

pub unsafe fn control_memory(
    addr0: usize,
    addr1: usize,
    size: usize,
    op: mem::MemoryOperation,
    permission: mem::MemoryPermission,
) -> Result<usize, ResultCode> {
    let mut dest: usize = 0;
    svcControlMemory(
        &mut dest as *mut usize,
        addr0,
        addr1,
        size,
        op.0,
        permission as u32,
    )?;

    Ok(dest)
}

pub unsafe fn get_system_info(sysinfo_type: u32, param: i32) -> i64 {
    let mut out: i64 = 0;
    svcGetSystemInfo(&mut out as *mut i64, sysinfo_type, param);
    out
}
