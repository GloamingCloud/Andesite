export fn kernelEntry() callconv(.naked) noreturn {
    while (true)
        asm volatile ("hlt");
}
