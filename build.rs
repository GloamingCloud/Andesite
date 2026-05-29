use std::{path::PathBuf, process::Command};

use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    fetch_ovmf();
    build_bootloader();
    build_kernel();
}

fn fetch_ovmf() {
    let ovmf = Prebuilt::fetch(Source::LATEST, "vm/ovmf").expect("Failed to fetch prebuilt");
    println!(
        "cargo::rustc-env=OVMF_CODE={}",
        ovmf.get_file(Arch::X64, FileType::Code).display()
    );
    println!(
        "cargo::rustc-env=OVMF_VARS={}",
        ovmf.get_file(Arch::X64, FileType::Vars).display()
    );
}

fn build_bootloader() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());

    println!("cargo::rerun-if-changed=bootloader");

    let mut cmd = Command::new(cargo);
    cmd.arg("build");
    cmd.arg("-p").arg("bootloader");
    cmd.arg("--target").arg("x86_64-unknown-uefi");
    cmd.arg("--target-dir").arg(&out_dir);
    cmd.arg("--release");

    let status = cmd.status().expect("failed to build bootloader");

    if status.success() {
        let bootloader = out_dir
            .join("x86_64-unknown-uefi")
            .join("release")
            .join("bootloader.efi");
        assert!(bootloader.exists(), "bootloader expected");
        println!("cargo::rustc-env=BOOTLOADER_OUT={}", bootloader.display());
    } else {
        panic!("failed to build bootloader");
    }
}

fn build_kernel() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());

    println!("cargo::rerun-if-changed=kernel");

    let mut cmd = Command::new(cargo);
    cmd.arg("build");
    cmd.arg("-p").arg("kernel");
    cmd.arg("--target").arg("x86_64-unknown-none");
    cmd.arg("--target-dir").arg(&out_dir);
    cmd.arg("--release");

    let status = cmd.status().expect("failed to build kernel");

    if status.success() {
        let kernel = out_dir
            .join("x86_64-unknown-none")
            .join("release")
            .join("kernel");
        println!("{}", kernel.display());
        assert!(kernel.exists(), "kernel expected");
        println!("cargo::rustc-env=KERNEL_OUT={}", kernel.display());
    } else {
        panic!("failed to build kernel");
    }
}
