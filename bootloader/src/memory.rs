use uefi::{
    boot::{MemoryDescriptor, MemoryType},
    mem::memory_map::MemoryMapIter,
};
use x86_64::{
    PhysAddr,
    structures::paging::{FrameAllocator, PhysFrame, Size4KiB},
};

pub struct UefiLegacyFrameAllocator<'a> {
    original: MemoryMapIter<'a>,
    memory_map_iter: MemoryMapIter<'a>,
    current_descriptor: Option<&'a MemoryDescriptor>,
    next_page: u64,
}

impl<'a> UefiLegacyFrameAllocator<'a> {
    pub fn new(memory_map_iter: MemoryMapIter<'a>) -> Self {
        Self {
            original: memory_map_iter.clone(),
            memory_map_iter,
            current_descriptor: None,
            next_page: 0,
        }
    }

    pub fn max_physical_address(&self) -> u64 {
        self.original
            .clone()
            .map(|d| d.phys_start + d.page_count * 4096)
            .max()
            .unwrap_or_default()
    }
}

unsafe impl<'a> FrameAllocator<Size4KiB> for UefiLegacyFrameAllocator<'a> {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        loop {
            if let Some(desc) = self.current_descriptor {
                let start = desc.phys_start;
                let end = start + (desc.page_count * 4096);

                if self.next_page < start {
                    self.next_page = start;
                }

                if self.next_page < 0x200000 {
                    self.next_page = 0x200000;
                }

                if self.next_page < end {
                    let frame = PhysFrame::containing_address(PhysAddr::new(self.next_page));
                    self.next_page += 4096;
                    return Some(frame);
                }
            }

            self.current_descriptor = self
                .memory_map_iter
                .find(|d| d.ty == MemoryType::CONVENTIONAL);

            if self.current_descriptor.is_none() {
                return None;
            }
        }
    }
}
