#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

mod idt;
mod mem;

use common::{ErrorType, KernelParameters, Result};

#[unsafe(no_mangle)]
pub extern "sysv64" fn _start(params: KernelParameters) -> ! {
    kmain(params).unwrap();

    panic!("kmain should not exit");
}

fn kmain(params: KernelParameters) -> Result<()> {
    common::logger::init_logger()?;
    init_uefi_rt_table(params.system_table)?;

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
