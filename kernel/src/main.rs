#![no_std]
#![no_main]

use common::KernelParameters;

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "sysv64" fn _start(kernel_parameters: KernelParameters) -> usize {
    kernel_parameters.sth
}
