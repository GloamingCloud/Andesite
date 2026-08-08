const std = @import("std");
const uefi = std.os.uefi;
const log = std.log.scoped(.bootloader);
const elf = std.elf;

const blog = @import("logger.zig");
const arch = @import("lib.zig").arch;
const defs = @import("defs.zig");

pub const std_options = std.Options{
    .logFn = blog.log,
};

const BootloaderError = error{
    NoOutput,
    NoBootService,
    SetPageTableWritableFailed,
    SimpleFileSystemNotFound,
    FailOpeningVolume,
    FailOpeningFile,
    NoMemory,
    FailFetchingInfo,
    IO,
    ErrorParsing,
    CleaningUp,
};

const KernelEntryType = fn (defs.BootInfo) callconv(.{ .x86_64_win = .{} }) noreturn;

fn bootloader() !void {
    const console_output = uefi.system_table.con_out orelse return BootloaderError.NoOutput;
    console_output.clearScreen() catch unreachable;

    blog.init(console_output);
    log.info("into bootloader, logger working", .{});

    const boot_service = uefi.system_table.boot_services orelse return BootloaderError.NoBootService;

    arch.page.setLv4Writable(boot_service) catch return BootloaderError.SetPageTableWritableFailed;

    const simple_file_system_protocol = boot_service.locateProtocol(
        uefi.protocol.SimpleFileSystem,
        null,
    ) catch null orelse return BootloaderError.SimpleFileSystemNotFound;

    const root_dir = simple_file_system_protocol.openVolume() catch return BootloaderError.FailOpeningVolume;
    errdefer root_dir.close() catch unreachable;

    const kernel_executable_path: [*:0]const u16 = std.unicode.utf8ToUtf16LeStringLiteral("kernel");
    const kernel_file = root_dir.open(
        kernel_executable_path,
        .read,
        .{
            .read_only = true,
            .directory = false,
        },
    ) catch return BootloaderError.FailOpeningFile;
    errdefer kernel_file.close() catch unreachable;

    const kernel_file_info_size = kernel_file.getInfoSize(.file) catch return BootloaderError.FailFetchingInfo;
    const kernel_file_info_buffer = boot_service.allocatePool(
        .loader_data,
        kernel_file_info_size,
    ) catch return BootloaderError.NoMemory;
    errdefer boot_service.freePool(kernel_file_info_buffer.ptr) catch unreachable;
    const kernel_file_info = kernel_file.getInfo(.file, kernel_file_info_buffer) catch return BootloaderError.FailFetchingInfo;

    log.debug("kernel file size: {}", .{kernel_file_info.file_size});

    const kernel_buffer = boot_service.allocatePool(
        .loader_data,
        kernel_file_info.file_size,
    ) catch return BootloaderError.NoMemory;
    errdefer boot_service.freePool(kernel_buffer.ptr) catch unreachable;
    _ = kernel_file.read(kernel_buffer) catch return BootloaderError.IO;

    var fbs = std.Io.Reader.fixed(kernel_buffer);
    const elf_header = elf.Header.read(&fbs) catch return BootloaderError.ErrorParsing;

    const Addr = elf.Elf64.Addr;
    var kernel_start_virt: Addr = std.math.maxInt(Addr);
    var kernel_start_phys: Addr align(4096) = std.math.maxInt(Addr);
    var kernel_end_phys: Addr = 0;

    var iter = elf_header.iterateProgramHeadersBuffer(kernel_buffer);

    while (true) {
        const phdr = try iter.next() orelse break;
        if (phdr.p_type != @intFromEnum(elf.PT.LOAD)) continue;
        if (phdr.p_paddr < kernel_start_phys) kernel_start_phys = phdr.p_paddr;
        if (phdr.p_vaddr < kernel_start_virt) kernel_start_virt = phdr.p_vaddr;
        if (phdr.p_paddr + phdr.p_memsz > kernel_end_phys) kernel_end_phys = phdr.p_paddr + phdr.p_memsz;
    }

    const pages_4kib = (kernel_end_phys - kernel_start_phys + 4095) / 4096;
    log.info("kernel image: 0x{X:0>16} - 0x{X:0>16} (0x{X} pages)", .{ kernel_start_phys, kernel_end_phys, pages_4kib });

    _ = boot_service.allocatePages(
        .{
            .address = @ptrFromInt(kernel_start_phys),
        },
        .loader_data,
        pages_4kib,
    ) catch return BootloaderError.NoMemory;

    for (0..pages_4kib) |i| {
        try arch.page.map4kTo(
            kernel_start_virt + 4096 * i,
            kernel_start_phys + 4096 * i,
            .read_write,
            boot_service,
        );
    }
    log.info("mapped memory for kernel image.", .{});
    log.info("loading kernel", .{});

    iter = elf_header.iterateProgramHeadersBuffer(kernel_buffer);
    while (true) {
        const phdr = try iter.next() orelse break;
        if (phdr.p_type != @intFromEnum(elf.PT.LOAD)) continue;

        const segment: [*]u8 = @ptrFromInt(phdr.p_vaddr);
        @memcpy(segment, kernel_buffer[phdr.p_offset .. phdr.p_offset + phdr.p_memsz]);

        log.info("  segment @ 0x{X:0>16} - 0x{X:0>16}", .{ phdr.p_vaddr, phdr.p_vaddr + phdr.p_memsz });

        const zero_count = phdr.p_memsz - phdr.p_filesz;
        if (zero_count > 0) {
            boot_service._setMem(@ptrFromInt(phdr.p_vaddr + phdr.p_filesz), zero_count, 0);
        }
    }
    log.info("kernel entry: 0x{X:0>16}", .{elf_header.entry});
    const kernel_entry: *KernelEntryType = @ptrFromInt(elf_header.entry);

    // cleaning up
    boot_service.freePool(kernel_buffer.ptr) catch return BootloaderError.CleaningUp;
    boot_service.freePool(kernel_file_info_buffer.ptr) catch return BootloaderError.CleaningUp;
    kernel_file.close() catch return BootloaderError.CleaningUp;
    root_dir.close() catch return BootloaderError.CleaningUp;

    log.info("exiting boot services", .{});
    const memory_map_info = try boot_service.getMemoryMapInfo();
    const memory_map_buffer = try boot_service.allocatePool(
        .loader_data,
        memory_map_info.descriptor_size * (memory_map_info.len + 1),
    );
    const memory_map = try boot_service.getMemoryMap(memory_map_buffer);

    try boot_service.exitBootServices(uefi.handle, memory_map.info.key);

    const boot_info = defs.BootInfo{
        .memory_map = defs.MemoryMap{
            .key = memory_map.info.key,
            .descriptor_size = memory_map.info.descriptor_size,
            .descriptor_version = memory_map.info.descriptor_version,
            .len = memory_map.info.len,
            .descriptors = @ptrCast(@alignCast(memory_map.ptr)),
        },
    };

    kernel_entry(boot_info);

    unreachable;
}

pub fn main() uefi.Status {
    bootloader() catch |err| {
        log.err("bootloader failure: {s}", .{@errorName(err)});
        return .aborted;
    };

    unreachable;
}
