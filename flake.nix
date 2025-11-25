{
  description = "Nix flake for the ZMK layout project (dev shell; build with Cargo)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      fenix,
      nixpkgs,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };

        lib = pkgs.lib;
        isLinux = pkgs.stdenv.isLinux;
        isDarwin = pkgs.stdenv.isDarwin;

        fenixPkgs = fenix.packages.${system};

        # Host toolchain (Linux/macOS) with the usual components
        fenixHostToolchain = fenixPkgs.complete.withComponents [
          "cargo"
          "clippy"
          "rust-src"
          "rustc"
          "rustfmt"
          "rust-analyzer"
        ];

        # Cross stdlib for Windows GNU target
        fenixWindowsStd = fenixPkgs.targets.x86_64-pc-windows-gnu.latest.rust-std;

        # Combined toolchain: host + Windows std
        fenixToolchain = fenixPkgs.combine [
          fenixHostToolchain
          fenixWindowsStd
        ];

        # Winpthreads (libpthread.a) for Windows GNU cross-linking
        mingwPthreads = pkgs.pkgsCross.mingwW64.windows.pthreads;

        nativeBuildInputs = [
          pkgs.pkg-config
        ];

        commonDevPackages = [
          fenixToolchain
          fenixPkgs.rust-analyzer

          pkgs.cargo-edit
          pkgs.cargo-deny
          pkgs.cargo-audit
          pkgs.cargo-ndk
          pkgs.cargo-cross

          pkgs.pkg-config
          pkgs.protobuf
          pkgs.openssl

          # MinGW cross-compiler: provides x86_64-w64-mingw32-gcc/ar, etc.
          pkgs.pkgsCross.mingwW64.stdenv.cc
          mingwPthreads
        ];

        linuxDevPackages =
          if isLinux then
            [
              pkgs.cargo-tarpaulin
            ]
          else
            [ ];

        darwinDevPackages =
          if isDarwin then
            [
              pkgs.libiconv
            ]
          else
            [ ];

      in
      {
        # Dev shell: use this, then build with `cargo` directly
        devShells.default = pkgs.mkShell {
          packages = commonDevPackages ++ linuxDevPackages ++ darwinDevPackages;

          inherit nativeBuildInputs;

          env = {
            CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = "x86_64-w64-mingw32-gcc";
            CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS = "-L native=${mingwPthreads}/lib";
            CC_x86_64_pc_windows_gnu = "x86_64-w64-mingw32-gcc";
            AR_x86_64_pc_windows_gnu = "x86_64-w64-mingw32-ar";
          };

          # Optional: small hint when entering the shell
          shellHook = ''
            echo "Rust dev shell (fenix + x86_64-pc-windows-gnu)."
            echo "  Linux build:   cargo build"
            echo "  Windows build: cargo build --target x86_64-pc-windows-gnu"
          '';
        };

        # Formatter for Nix files
        formatter = pkgs.alejandra;
      }
    );
}
