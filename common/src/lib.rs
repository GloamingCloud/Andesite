#![no_std]

use uefi::mem::memory_map::{MemoryMap, MemoryMapMeta, MemoryMapOwned, MemoryMapRef};

pub mod logger;

#[repr(C)]
pub struct MemMap {
    pub ptr: *const u8,
    pub size: usize,
    pub desc_size: usize,
    pub desc_version: u32,
}

impl MemMap {
    pub fn new(memory_map: &MemoryMapOwned) -> Self {
        let ptr = memory_map.buffer().as_ptr();
        let size = memory_map.meta().map_size;
        let desc_size = memory_map.meta().desc_size;
        let desc_version = memory_map.meta().desc_version;

        Self { ptr, size, desc_size, desc_version }
    }

    pub fn as_ref(&self) -> Result<MemoryMapRef<'_>> {
        let meta = MemoryMapMeta {
            map_size: self.size,
            desc_size: self.desc_size,
            map_key: Default::default(),
            desc_version: self.desc_version,
        };
        let slice = unsafe { core::slice::from_raw_parts(self.ptr, self.size) };
        MemoryMapRef::new(slice, meta).map_err(|_| ErrorType::InvalidData)
    }
}

#[repr(C)]
pub struct KernelParameters {
    pub system_table: *const core::ffi::c_void,
    pub memory_map: MemMap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorType {
    NullParameter,
    InvalidData,
}

pub type Result<T> = core::result::Result<T, ErrorType>;
