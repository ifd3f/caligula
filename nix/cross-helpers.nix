host:
{ nixpkgs, naersk, rust-overlay, ... }:
let
  pkgs = import nixpkgs {
    system = host;
    overlays = [ rust-overlay.overlays.default ];
  };
  lib = pkgs.lib;
  hostInfo = lib.systems.parse.mkSystemFromString host;
in rec {
  supportedSystems = if hostInfo.kernel.name == "linux" then [
    "aarch64-linux"
    "x86_64-linux"
  ] else if hostInfo.kernel.name == "darwin" then [
    "aarch64-darwin"
    "x86_64-darwin"
  ] else
    throw "unsupported host system ${host}";

  forTarget = target:
    let
      targetInfo = lib.systems.parse.mkSystemFromString target;

      buildCfg = if targetInfo.kernel.name == "linux" then {
        rustTarget = "${targetInfo.cpu.name}-unknown-linux-musl";
        platformDeps = [ ];
        cross = pkgs.pkgsCross."${targetInfo.cpu.name}-multiplatform";
      } else if targetInfo.kernel.name == "darwin" then {
        rustTarget = "${targetInfo.cpu.name}-apple-darwin";
        platformDeps = with pkgs.darwin.apple_sdk.frameworks; [
          Cocoa
          IOKit
          Foundation
          DiskArbitration
        ];
        cross = pkgs.pkgsCross."${targetInfo.cpu.name}-darwin";
      } else
        throw "unsupported target system ${target}";

      targetLinkerEnvName = "CARGO_TARGET_${
          builtins.replaceStrings [ "-" ] [ "_" ]
          (lib.toUpper buildCfg.rustTarget)
        }_LINKER";

      rust-toolchain = pkgs.rust-bin.stable.latest.default.override {
        targets = [ buildCfg.rustTarget ];
      };
      rust-toolchain-dev = rust-toolchain.override {
        extensions = [ "rust-src" "rust-analyzer" ];
      };
      naersk' = pkgs.callPackage naersk {
        cargo = rust-toolchain;
        rustc = rust-toolchain;
      };

      caligula = with pkgs;
        naersk'.buildPackage ({
          src = ../.;
          doCheck = true;
          buildInputs = buildCfg.platformDeps;
          CARGO_BUILD_TARGET = buildCfg.rustTarget;
        } // (if host != target then {
          "${targetLinkerEnvName}" = "${buildCfg.cross.gcc}/bin/ld";
        } else
          { }));

    in rec {
      inherit rust-toolchain-dev caligula;
      inherit (buildCfg) platformDeps rustTarget;
    };

  caligulaPackages = lib.listToAttrs (lib.forEach supportedSystems (target: {
    name = "caligula-${target}";
    value = (forTarget target).caligula;
  }));
}
