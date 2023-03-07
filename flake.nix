{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, naersk, rust-overlay }:
    let
      supportedSystems =
        [ "aarch64-darwin" "aarch64-linux" "x86_64-darwin" "x86_64-linux" ];
    in flake-utils.lib.eachSystem supportedSystems (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        lib = pkgs.lib;

        getTargetInfo = target:
          let sysinfo = lib.systems.parse.mkSystemFromString target;
          in if sysinfo.kernel.name == "linux" then {
            # On Linux, we want to only support musl.
            rustTarget = "${sysinfo.cpu.name}-unknown-linux-musl";
            platformDeps = [ ];
            cross = pkgs.pkgsCross."${sysinfo.cpu.name}-multiplatform";
          } else if sysinfo.kernel.name == "darwin" then {
            rustTarget = "${sysinfo.cpu.name}-apple-darwin";
            platformDeps = with pkgs.darwin.apple_sdk.frameworks; [
              Cocoa
              IOKit
              Foundation
              DiskArbitration
            ];
            cross = pkgs.pkgsCross."${sysinfo.cpu.name}-darwin";
          } else
            throw "unsupported system ${system}";

        makeToolchainForTarget = target: rec {
          targetInfo = getTargetInfo target;
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
          targetLinkerEnvName = "CARGO_TARGET_${
              builtins.replaceStrings [ "-" ] [ "_" ]
              (lib.toUpper targetInfo.rustTarget)
            }_LINKER";
        };

        makePackageForTarget = target:
          let tc = makeToolchainForTarget target;
          in with pkgs;
          tc.naersk'.buildPackage ({
            src = ./.;
            doCheck = true;
            buildInputs = tc.targetInfo.platformDeps;
            CARGO_BUILD_TARGET = tc.targetInfo.rustTarget;
          } // (if system != target then {
            "${tc.targetLinkerEnvName}" = "${tc.targetInfo.cross.gcc}/bin/ld";
          } else
            { }));

        caligulaPackages = lib.listToAttrs (lib.forEach supportedSystems
          (target: {
            name = "caligula-${target}";
            value = makePackageForTarget target;
          }));
      in {
        packages = {
          default = self.packages."${system}".caligula;

          caligula = self.packages."${system}"."caligula-${system}";
        } // caligulaPackages;

        devShell = let tc = makeToolchainForTarget system;
        in with pkgs;
        mkShell {
          buildInputs = [ nixfmt tc.rust-toolchain-dev ]
            ++ tc.targetInfo.platformDeps;
          CARGO_BUILD_TARGET = tc.targetInfo.rustTarget;
        };
      });
}
