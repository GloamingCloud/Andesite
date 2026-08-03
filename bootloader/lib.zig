const builtin = @import("builtin");

pub const arch = switch (builtin.cpu.arch) {
    .x86_64 => @import("arch/x86/arch.zig"),
    else => @compileError("unsupported architecture"),
};
