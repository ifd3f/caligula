{ nixpkgs, naersk, rust-overlay, ... }:
host:
let
  lib = nixpkgs.lib;
  hostInfo = lib.systems.parse.mkSystemFromString host;
in rec {
  pkgs = import nixpkgs {
    system = host;
    overlays = [ rust-overlay.overlays.default ];
  };

  baseToolchain = pkgs.rust-bin.stable.latest.default;

  supportedSystems = if hostInfo.kernel.name == "linux" then [
    "aarch64-linux"
    "x86_64-linux"
  ] else if host == "x86_64-darwin" then [
    "aarch64-darwin"
    "x86_64-darwin"
  ] else if host == "aarch64-darwin" then
    [ "aarch64-darwin" ]
  else
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
      pkgs = import nixpkgs { system = host; };

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
      rust-toolchain =
        baseToolchain.override { targets = [ buildCfg.rustTarget ]; };

      naersk' = pkgs.callPackage naersk {
        cargo = rust-toolchain;
        rustc = rust-toolchain;
      };

      crossParams = if host == target then {
        cc = pkgs.stdenv.cc;
        extraBuildEnv = { };
      } else rec {
        cc = pkgsCross.stdenv.cc;
        extraBuildEnv = {
          "${targetLinkerEnvName}" = "${cc}/bin/${buildCfg.rustTarget}-ld";

          "CC_${builtins.replaceStrings [ "-" ] [ "_" ] buildCfg.rustTarget}" =
            "${cc}/bin/${buildCfg.rustTarget}-cc";
        };
      };

      buildInputs = buildCfg.platformDeps ++ [ crossParams.cc ];

      # The actual package
      caligula = with pkgs;
        naersk'.buildPackage ({
          src = ../.;
          doCheck = host == target;
          propagatedBuildInputs = [ crossParams.cc ];
          inherit buildInputs;
          CARGO_BUILD_TARGET = buildCfg.rustTarget;
        } // crossParams.extraBuildEnv);

    in {
      inherit pkgs pkgsCross rust-toolchain caligula buildInputs;
      inherit (buildCfg) platformDeps rustTarget;
      inherit (crossParams) extraBuildEnv;

      naersk = naersk';
    };

  crossCompileDevShell = let
    rust = baseToolchain.override {
      extensions = [ "rust-src" "rust-analyzer" ];
      targets = (map (target: (forTarget target).rustTarget) supportedSystems);
    };

    extraEnv = lib.foldl' (a: b: a // b) { }
      (map (target: (forTarget target).extraBuildEnv) supportedSystems);

  in pkgs.mkShell ({
    buildInputs = [ rust ]
      ++ lib.concatMap (target: (forTarget target).buildInputs)
      supportedSystems;
  } // extraEnv);

  caligulaPackages = lib.listToAttrs (lib.forEach supportedSystems (target: {
    name = "caligula-${target}";
    value = (forTarget target).caligula;
  }));
}
