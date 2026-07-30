use uefi::{
    boot::{MemoryDescriptor, MemoryType},
    mem::memory_map::MemoryMapIter,
};
use x86_64::{
    PhysAddr,
    structures::paging::{FrameAllocator, PhysFrame, Size4KiB},
};
