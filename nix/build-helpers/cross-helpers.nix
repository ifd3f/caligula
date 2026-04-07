# Helpers for making a cross-compiling toolchain for caligula.
# To work, it needs pkgs to have rust-overlay applied to it.
{
  system,
  target,

  lib,
  stdenv,
  callPackage,

  # rusty things
  baseToolchain, # an item like pkgs.rust-bin.stable.latest.default;
  rust,
  naersk,

  # needed for macos
  apple-sdk,

  # needed to construct pkgsCross
  path,
  overlays,
  config,
  ...
}:
let
  # The caligula source code itself.
  src = builtins.path {
    path = ../../.;
    name = "caligula-src";

    # There's lots of extraneous files that can cause a cache miss. Hide them.
    filter =
      path: type:
      # path is of the format /nix/store/hash-whatever/Cargo.toml
      let
        rootDirName = builtins.elemAt (lib.splitString "/" path) 4;
      in
      builtins.elem rootDirName [
        ".cargo"
        "native"
        "src"

        "build.rs"
        "Cargo.lock"
        "Cargo.toml"
      ];
  };

  # Determine some platform-specific parameters.
  # rustTarget is the triple, and platformDeps is what additional build deps
  # the target platform needs during compilation.
  targetInfo = lib.systems.parse.mkSystemFromString target;
  buildCfg =
    if targetInfo.kernel.name == "linux" then
      {
        rustTarget = "${targetInfo.cpu.name}-unknown-linux-musl";
        platformDeps = [ ];
      }
    else if targetInfo.kernel.name == "darwin" then
      {
        rustTarget = "${targetInfo.cpu.name}-apple-darwin";
        platformDeps = [ apple-sdk ];
      }
    else
      throw "unsupported target system ${target}";

  # Determine name for the linker env var to pass to cargo
  targetLinkerEnvName = "CARGO_TARGET_${
    builtins.replaceStrings [ "-" ] [ "_" ] (lib.toUpper buildCfg.rustTarget)
  }_LINKER";

  crossParams =
    if system == target then # not cross compiling
      {
        cc = stdenv.cc;

        # needed to do macro expansions
        extraBuildEnv = {
          RUST_SRC_PATH = "${rust.packages.stable.rustPlatform.rustLibSrc}";
        };
      }
    # we are cross compiling
    else
      rec {
        # reconstruct pkgsCross with all its modifications, but for the specific target
        pkgsCross = import path {
          inherit overlays config;
          localSystem = system;

          # We specifically need to use this because we need the triple including the
          # -musl/-gcc bit, and not just the system double.
          crossSystem.config = buildCfg.rustTarget;
        };

        # use cross's cc
        cc = pkgsCross.stdenv.cc;

        # these environment variables allow cargo to cross-compile
        extraBuildEnv = {
          "${targetLinkerEnvName}" = "${cc}/bin/${buildCfg.rustTarget}-ld";

          "CC_${builtins.replaceStrings [ "-" ] [ "_" ] buildCfg.rustTarget}" =
            "${cc}/bin/${buildCfg.rustTarget}-cc";
        };
      };
in
rec {
  inherit (crossParams) cc extraBuildEnv;
  inherit (buildCfg) rustTarget platformDeps;

  # Construct a rust toolchain that runs on the host
  rust-toolchain = baseToolchain.override { targets = [ buildCfg.rustTarget ]; };

  # Instantiate naersk
  naersk' = callPackage naersk {
    cargo = rust-toolchain;
    rustc = rust-toolchain;
  };

  buildInputs = buildCfg.platformDeps ++ [ crossParams.cc ];

  # The actual package
  caligula = naersk'.buildPackage (
    {
      inherit src;
      doCheck = system == target;
      propagatedBuildInputs = [ crossParams.cc ];
      inherit buildInputs;
      cargoBuildOptions = args: args ++ [ "--locked" ];
      CARGO_BUILD_TARGET = buildCfg.rustTarget;
    }
    // crossParams.extraBuildEnv
  );
}
