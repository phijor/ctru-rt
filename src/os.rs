#[derive(Debug)]
pub enum MemoryRegion {
    Application = 1,
    System = 2,
    Base = 3,
}

impl MemoryRegion {
    pub fn size(&self) -> usize {
        unimplemented!()
    }
}
