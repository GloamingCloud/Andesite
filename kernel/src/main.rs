#![no_std]
#![no_main]

use common::KernelParameters;
use uefi::{Status, runtime::ResetType};

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "sysv64" fn _start(params: KernelParameters) -> usize {
    if !params.system_table.is_null() {
        unsafe {
            uefi::table::set_system_table(params.system_table.cast());
        }
    } else {
        return 1;
    }

    uefi::runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None);
}
