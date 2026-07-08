#![no_std]
#![no_main]

use core::panic::PanicInfo;

use common::KernelParameters;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "sysv64" fn _start(kernel_parameters: KernelParameters) -> usize {
    return kernel_parameters.sth;
}
