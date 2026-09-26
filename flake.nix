{
  description = "Calvin: a language, embedded compiler, and runtime";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane = {
      url = "github:ipetkov/crane";
    };
  };

  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      
      perSystem = { pkgs, system, ... }: 
        let
          overlays = [ (import inputs.rust-overlay) ];
          pkgs_with_rust = import inputs.nixpkgs {
            inherit system overlays;
          };
          rustToolchain = pkgs_with_rust.rust-bin.nightly.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
          };
          craneLib = (inputs.crane.mkLib pkgs_with_rust).overrideToolchain rustToolchain;
          
          hobFilter = path: _type: builtins.match ".*\\.hob$" path != null;
          hobOrCargo = path: type:
            (hobFilter path type) || (craneLib.filterCargoSources path type);

          src = pkgs_with_rust.lib.cleanSourceWith {
            src = craneLib.path ./.;
            filter = hobOrCargo;
          };
          commonArgs = {
            pname = "calvin";
            version = "0.1.0";
            inherit src;
            strictDeps = true;
            buildInputs = with pkgs_with_rust; [
              llvmPackages_18.llvm
              llvmPackages_18.bintools
              llvmPackages_18.clang
              zstd
              libffi
              libxml2
            ];
            nativeBuildInputs = [ pkgs_with_rust.pkg-config ];
            LLVM_SYS_180_PREFIX = pkgs_with_rust.llvmPackages_18.llvm.dev;
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          calvin = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
          });
        in {
          packages = {
            default = calvin;
            calvin = calvin;
          };
          
          apps = {
            ci = {
              type = "app";
              program = "${calvin}/bin/ci";
            };
            default = {
              type = "app";
              program = "${calvin}/bin/ci";
            };
          };

          devShells.default = pkgs_with_rust.mkShell {
            buildInputs = with pkgs_with_rust; [
              rustToolchain
              llvmPackages_18.llvm
              llvmPackages_18.bintools
              llvmPackages_18.clang
              typst
              cargo-nextest
              cargo-fuzz
              cargo-audit
              zstd
              libffi
              libxml2
            ];
            shellHook = ''
              export LLVM_SYS_180_PREFIX=${pkgs_with_rust.llvmPackages_18.llvm.dev}
              export RUST_BACKTRACE=1
            '';
          };
        };
    };
}
