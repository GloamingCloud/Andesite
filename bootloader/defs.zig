const std = @import("std");
const uefi = std.os.uefi;

pub const MemoryMap = extern struct {
    key: uefi.tables.MemoryMapKey,
    descriptor_size: usize,
    descriptor_version: u32,
    len: usize,
    descriptors: [*]uefi.tables.MemoryDescriptor,
};

pub const BootInfo = extern struct {
    memory_map: MemoryMap,
};
