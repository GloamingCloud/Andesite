#![no_std]
#![no_main]

use common::{ErrorType, KernelParameters, Result};
use uefi::{boot::MemoryType, mem::memory_map::MemoryMap};

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
    let mut pages: usize = 0;
    for desc in memory_map_ref.entries() {
        log::info!("{:?}", desc);
        if desc.ty == MemoryType::CONVENTIONAL
            || desc.ty == MemoryType::BOOT_SERVICES_CODE
            || desc.ty == MemoryType::BOOT_SERVICES_DATA
        {
            pages += desc.page_count as usize;
        }
    }
    log::info!("{} pages available, total of {} MiB memory avail.", pages, pages >> 8);

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
