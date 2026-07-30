const std = @import("std");
const uefi = std.os.uefi;
const log = std.log.scoped(.bootloader);

const blog = @import("logger.zig");

pub const std_options = std.Options{
    .logFn = blog.log,
};

pub fn main() uefi.Status {
    const console_output = uefi.system_table.con_out orelse return .aborted;
    console_output.clearScreen() catch unreachable;

    blog.init(console_output);

    log.debug("into bootloader", .{});

    while (true) asm volatile ("hlt");
    return .success;
}
