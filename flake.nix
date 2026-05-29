{
  description = "Andesite, a toy operating system";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
          config.allowUnfree = true;
        };
        rustToolchain = pkgs.rust-bin.nightly.latest.default.override {
          targets = [
            "x86_64-unknown-uefi"
            "x86_64-unknown-none"
          ];
          extensions = [ "rust-src" ];
        };
      in
      {
        devShells.default =
          with pkgs;
          mkShell {
            buildInputs = [
              qemu
              rustToolchain

              jetbrains.rust-rover
            ];

            shellHook = ''
                echo "Rust version: $(rustc --version)"
                echo "QEMU version: $(qemu-system-x86_64 --version | head -n 1)"

                ln -sfn ${rustToolchain}/lib ~/.rust-rover/toolchain
              ln -sfn ${rustToolchain}/bin ~/.rust-rover/toolchain

              export RUST_SRC_PATH="$HOME/.rust-rover/toolchain/lib/rustlib/src/rust/library"
            '';
          };
      }
    );
}
