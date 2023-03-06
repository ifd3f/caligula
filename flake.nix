{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, naersk, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        lib = pkgs.lib;

        # On Linux, we want to only support one target.
        sysinfo = lib.systems.parse.mkSystemFromString system;
        targetInfo = if sysinfo.kernel.name == "linux" then {
          rustTarget = "${sysinfo.cpu.name}-unknown-linux-musl";
          platformDeps = [ ];
        } else if sysinfo.kernel.name == "darwin" then {
          rustTarget = "${sysinfo.cpu.name}-apple-darwin";
          platformDeps = with pkgs.darwin.apple_sdk.frameworks; [
            Cocoa
            IOKit
            Foundation
            DiskArbitration
          ];
        } else
          throw "unknown system ${system}";

        rust-toolchain = pkgs.rust-bin.stable.latest.default.override {
          targets = [ targetInfo.rustTarget ];
        };
        rust-toolchain-dev = rust-toolchain.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
        naersk' = pkgs.callPackage naersk {
          cargo = rust-toolchain;
          rustc = rust-toolchain;
        };
      in {
        packages.default = with pkgs;
          naersk'.buildPackage {
            src = "${self}";
            doCheck = true;
            buildInputs = targetInfo.platformDeps;
            CARGO_BUILD_TARGET = targetInfo.rustTarget;
          };

        devShell = with pkgs;
          mkShell {
            buildInputs = [ nixfmt rust-toolchain-dev ]
              ++ targetInfo.platformDeps;
            CARGO_BUILD_TARGET = targetInfo.rustTarget;
          };
      });
}
