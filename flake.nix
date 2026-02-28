{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    fenix.url = "github:nix-community/fenix";
  };

  outputs =
    {
      self,
      nixpkgs,
      fenix,
    }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      toolchain = fenix.packages.${system}.fromToolchainFile {
        file = ./rust-toolchain.toml;
        sha256 = "sha256-vhDlEebuggsbvmo60PHo61saUFGasTQiOS4+hRgwvsY=";
      };
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        buildInputs = with pkgs; [
          toolchain
          qemu
        ];
        shellHook = ''
          echo "Rust version: $(rustc --version)"
          echo "QEMU version: $(qemu-system-x86_64 --version | head -n 1)"
        '';
      };
    };
}
