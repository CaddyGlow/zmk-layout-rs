{
  description = "Nix flake for the ZMK layout project";

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
          config = {
            allowUnfree = true;
          };
        };
        lib = pkgs.lib;
        isLinux = pkgs.stdenv.isLinux;
        isDarwin = pkgs.stdenv.isDarwin;
        fenixPkgs = fenix.packages.${system};
        fenixToolchain = fenixPkgs.complete.withComponents [
          "cargo"
          "clippy"
          "rust-src"
          "rustc"
          "rustfmt"
          "cargo-cross"
        ];
        rustPlatform = pkgs.makeRustPlatform {
          cargo = fenixToolchain;
          rustc = fenixToolchain;
        };
        cargoToml = lib.importTOML ./Cargo.toml;
        crateName = cargoToml.package.name;
        crateVersion = cargoToml.package.version;

        projectDescription = "ZMK layout editor";

        cratePackage = rustPlatform.buildRustPackage {
          pname = crateName;
          version = crateVersion;
          src = lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoHash = lib.fakeSha256;
          inherit nativeBuildInputs;
          buildInputs = [ ];
          meta = with lib; {
            description = projectDescription;
            license = licenses.mit;
            maintainers = [ ];
          };
        };

        # Cross-compilation helper function
        mkCrossPackage =
          {
            crossPkgs,
            targetTriple,
            targetName,
          }:
          let
            targetToolchain = fenixPkgs.combine [
              fenixPkgs.complete.cargo
              fenixPkgs.complete.rustc
              fenixPkgs.targets.${targetTriple}.latest.rust-std
            ];
            crossRustPlatform = crossPkgs.makeRustPlatform {
              cargo = targetToolchain;
              rustc = targetToolchain;
            };
          in
          crossRustPlatform.buildRustPackage {
            pname = "${crateName}-${targetName}";
            version = crateVersion;
            src = lib.cleanSource ./.;
            cargoLock.lockFile = ./Cargo.lock;
            cargoHash = lib.fakeSha256;

            # Don't include pkg-config for cross-compilation as it often fails
            # and isn't needed for static Rust binaries
            nativeBuildInputs = [ ];
            buildInputs = [ ];

            meta = with lib; {
              description = "${projectDescription} (${targetName})";
              license = licenses.mit;
              maintainers = [ ];
            };
          };

        nativeBuildInputs = [ pkgs.pkg-config ];
        commonDevPackages = [
          fenixToolchain
          fenixPkgs.rust-analyzer
          pkgs.cargo-edit
          pkgs.cargo-deny
          pkgs.cargo-audit
          pkgs.cargo-ndk
          pkgs.pkg-config
          pkgs.protobuf
          pkgs.openssl
        ];
        linuxDevPackages =
          if isLinux then
            [
              pkgs.cargo-tarpaulin
            ]
          else
            [ ];
        darwinDevPackages = if isDarwin then [ pkgs.libiconv ] else [ ];

      in
      {
        packages = {
          default = cratePackage;

          # Cross-platform builds
          # Windows
          windows-x86_64 = mkCrossPackage {
            crossPkgs = pkgs.pkgsCross.mingwW64;
            targetTriple = "x86_64-pc-windows-gnu";
            targetName = "windows-x86_64";
          };

          # macOS
          macos-aarch64 = mkCrossPackage {
            crossPkgs = pkgs.pkgsCross.aarch64-darwin;
            targetTriple = "aarch64-apple-darwin";
            targetName = "macos-aarch64";
          };
          macos-x86_64 = mkCrossPackage {
            crossPkgs = pkgs.pkgsCross.x86_64-darwin;
            targetTriple = "x86_64-apple-darwin";
            targetName = "macos-x86_64";
          };

          # Linux
          linux-x86_64 = mkCrossPackage {
            crossPkgs = pkgs.pkgsCross.gnu64;
            targetTriple = "x86_64-unknown-linux-gnu";
            targetName = "linux-x86_64";
          };
          linux-x86_64-musl = mkCrossPackage {
            crossPkgs = pkgs.pkgsCross.musl64;
            targetTriple = "x86_64-unknown-linux-musl";
            targetName = "linux-x86_64-musl";
          };
          linux-aarch64 = mkCrossPackage {
            crossPkgs = pkgs.pkgsCross.aarch64-multiplatform;
            targetTriple = "aarch64-unknown-linux-gnu";
            targetName = "linux-aarch64";
          };

          # Android builds for common architectures
        };

        apps.default = {
          type = "app";
          program = "${cratePackage}/bin/${crateName}";
        };

        devShells.default = pkgs.mkShell {
          packages = commonDevPackages ++ linuxDevPackages ++ darwinDevPackages;

          inherit nativeBuildInputs;

          shellHook = '''';
        };

        formatter = pkgs.alejandra;

        checks.build = cratePackage;
      }
    );
}
