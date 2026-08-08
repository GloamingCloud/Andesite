const bootloader = @import("bootloader");

extern const __stackguard_lower: [*]const u8;

export fn kernelEntry() callconv(.naked) noreturn {
    asm volatile (
        \\movq %[new_stack], %%rsp
        \\call kernelTrampoline
        :
        : [new_stack] "r" (@intFromPtr(&__stackguard_lower) - 0x10),
    );
}

export fn kernelTrampoline(boot_info: bootloader.BootInfo) callconv(.{ .x86_64_win = .{} }) noreturn {
    kernelMain(boot_info) catch {
        @panic("Exiting...");
    };

    unreachable;
}

fn kernelMain(boot_info: bootloader.BootInfo) !void {
    _ = boot_info;

    while (true) asm volatile ("hlt");
}
