{ nixpkgs, naersk, rust-overlay, ... }:
host:
let
  lib = nixpkgs.lib;
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
      # Determine some platform-specific parameters.
      targetInfo = lib.systems.parse.mkSystemFromString target;
      buildCfg = if targetInfo.kernel.name == "linux" then {
        rustTarget = "${targetInfo.cpu.name}-unknown-linux-musl";
        platformDeps = [ ];
      } else if targetInfo.kernel.name == "darwin" then {
        rustTarget = "${targetInfo.cpu.name}-apple-darwin";
        platformDeps = with pkgs.darwin.apple_sdk.frameworks; [
          Cocoa
          IOKit
          Foundation
          DiskArbitration
        ];
      } else
        throw "unsupported target system ${target}";

      # Construct pkgs (host = target = this system)
      pkgs = import nixpkgs {
        system = host;
        overlays = [ rust-overlay.overlays.default ];
      };

      # Construct pkgsCross (host = this system, target = target we want)
      pkgsCross = import nixpkgs {
        system = host;
        crossSystem.config = buildCfg.rustTarget;
        overlays = [ rust-overlay.overlays.default ];
      };

      # Determine name for the linker env var to pass to cargo
      targetLinkerEnvName = "CARGO_TARGET_${
          builtins.replaceStrings [ "-" ] [ "_" ]
          (lib.toUpper buildCfg.rustTarget)
        }_LINKER";

      # Construct a rust toolchain that runs on the host
      rust-toolchain = pkgs.rust-bin.stable.latest.default.override {
        targets = [ buildCfg.rustTarget ];
      };
      naersk' = pkgs.callPackage naersk {
        cargo = rust-toolchain;
        rustc = rust-toolchain;
      };

      extraBuildEnv = if host != target then {
        "${targetLinkerEnvName}" =
          "${pkgsCross.stdenv.cc}/bin/${buildCfg.rustTarget}-ld";
      } else
        { };

      # The actual package
      caligula = with pkgs;
        naersk'.buildPackage ({
          src = ../.;
          doCheck = host == target;
          buildInputs = buildCfg.platformDeps;
          CARGO_BUILD_TARGET = buildCfg.rustTarget;
        } // extraBuildEnv);

    in {
      inherit pkgs pkgsCross rust-toolchain caligula extraBuildEnv;
      inherit (buildCfg) platformDeps rustTarget;

      naersk = naersk';

      # The development toolchain config with IDE goodies
      rust-toolchain-dev = rust-toolchain.override {
        extensions = [ "rust-src" "rust-analyzer" ];
      };
    };

  caligulaPackages = lib.listToAttrs (lib.forEach supportedSystems (target: {
    name = "caligula-${target}";
    value = (forTarget target).caligula;
  }));
}
