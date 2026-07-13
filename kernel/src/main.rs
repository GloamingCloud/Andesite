#![no_std]
#![no_main]

mod mem;

use common::{ErrorType, KernelParameters, Result};
use uefi::mem::memory_map::MemoryMap;
use x86_64::{VirtAddr, structures::paging::{FrameAllocator, PageTable}};

use crate::mem::UefiLegacyFrameAllocator;

#[unsafe(no_mangle)]
pub extern "sysv64" fn _start(params: KernelParameters) -> ! {
    kmain(params).unwrap();

    panic!("kmain should not exit");
}

fn kmain(params: KernelParameters) -> Result<()> {
    common::logger::init_logger()?;
    init_uefi_rt_table(params.system_table)?;

    log::info!("in kernel now");

    let memory_map_ref = params.memory_map.as_ref()?;
    let mut frame_allocator = UefiLegacyFrameAllocator::new(memory_map_ref.entries());

    let new_kernel_page_table_frame = frame_allocator
        .allocate_frame()
        .expect("failed to allocate frame for new page table");
    let addr = VirtAddr::zero() + new_kernel_page_table_frame.start_address().as_u64();
    let ptr = addr.as_mut_ptr();
    unsafe { *ptr = PageTable::new() };
    let new_kernel_page_table = unsafe {&mut *ptr};

    loop {}
}

fn init_uefi_rt_table(system_table: *const core::ffi::c_void) -> Result<()> {
    if !system_table.is_null() {
        unsafe {
            uefi::table::set_system_table(system_table.cast());
        }
    } else {
        return Err(ErrorType::NullParameter);
    }

    Ok(())
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use core::arch::asm;

    unsafe { common::logger::LOGGER.get().map(|l| l.force_unlock()) };
    log::error!("{}", info);

    loop {
        unsafe { asm!("cli; hlt") };
    }
}
