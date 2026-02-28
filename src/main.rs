use std::{env, fs, process::Command};

fn main() {
    let current_dir = env::current_dir().expect("failed to get current directory");
    let vm = current_dir.join("vm");
    let esp = vm.join("esp");
    let boot = esp.join("efi").join("boot");

    if !vm.exists() {
        fs::create_dir(&vm).expect("failed to create VM directory");
    }
    if !esp.exists() {
        fs::create_dir(&esp).expect("failed to create ESP directory");
    }
    if !boot.exists() {
        fs::create_dir_all(&boot).expect("failed to create BOOT directory");
    }

    fs::copy(env!("BOOTLOADER_OUT"), boot.join("bootx64.efi"))
        .expect("failed to copy bootloader.efi");
    // fs::copy(env!("KERNEL_OUT"), esp.join("kernel")).expect("failed to copy kernel");

    let mut qemu_cmd = Command::new("qemu-system-x86_64");
    // qemu_cmd.args(vec!["-s", "-S"]);
    qemu_cmd.arg("-enable-kvm");
    qemu_cmd.arg("-no-reboot");
    qemu_cmd.args(&["-serial", "stdio"]);
    qemu_cmd.args(&[
        "-drive",
        &format!(
            "if=pflash,format=raw,readonly=on,file={}",
            env!("OVMF_CODE")
        ),
    ]);
    qemu_cmd.args(&[
        "-drive",
        &format!(
            "if=pflash,format=raw,readonly=on,file={}",
            env!("OVMF_VARS")
        ),
    ]);
    qemu_cmd.args(&[
        "-drive",
        &format!("format=raw,file=fat:rw:{}", esp.display()),
    ]);
    let status = qemu_cmd
        .status()
        .expect("failed to execute qemu system command");
    println!("qemu system command exited with status {}", status);
}
