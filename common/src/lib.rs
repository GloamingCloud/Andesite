#![no_std]

/// Opaque pointer to the UEFI System Table, for the kernel to register
/// with `uefi::table::set_system_table()` to access Runtime Services.
#[repr(C)]
pub struct KernelParameters {
    pub sth: usize,
    pub system_table: *const core::ffi::c_void,
}