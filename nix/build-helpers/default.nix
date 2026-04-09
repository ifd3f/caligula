# Helpers for building and cross-compiling Caligula.
{
  self,
  inputs,
  lib,
  ...
}:
let
  nixpkgs = inputs.nixpkgs;
in
{
  perSystem =
    { self', system, ... }:
    let
      # Build host's pkgs instance
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ self.overlays.rust-overlay ];
      };

      hostInfo = pkgs.stdenv.hostPlatform.parsed;

      # Calculate what we're able to build. This may be adjusted based on what's
      # working and what's not.
      supportedTargets =
        if hostInfo.kernel.name == "linux" then
          [
            "aarch64-linux"
            "x86_64-linux"
          ]
        else if system == "x86_64-darwin" then
          [
            # "aarch64-darwin" # Temporarily broken. TODO: fix
            "x86_64-darwin"
          ]
        else if system == "aarch64-darwin" then
          [ "aarch64-darwin" ]
        else
          throw "unsupported host system ${system}";

      baseToolchain = pkgs.rust-bin.stable.latest.default;

      perTarget =
        target:
        pkgs.callPackage ./cross-helpers.nix {
          inherit target baseToolchain;
          naersk = self.lib.naersk;
        };

      # All caligulas that are buildable by this system.
      caligulaPackages = lib.listToAttrs (
        lib.forEach supportedTargets (target: {
          name = "caligula-${target}";
          value = (perTarget target).caligula;
        })
      );
    in
    {
      packages = caligulaPackages // {
        caligula = self.packages.${system}."caligula-${system}";
        lint-script =
          with pkgs;
          writeShellApplication {
            name = "lint.sh";
            runtimeInputs = [
              bash
              baseToolchain
            ]
            # own system's build inputs
            ++ (perTarget system).buildInputs;
            text = builtins.readFile ../../scripts/lint.sh;
          };
      };

      # A devshell that has all of the helpers needed for cross-compilation on this system.
      devShells.cross =
        let
          rust = baseToolchain.override {
            extensions = [
              "rust-src"
              "rust-analyzer"
              "clippy"
            ];
            targets = (map (target: (perTarget target).rustTarget) supportedTargets);
          };

          extraEnv = lib.foldl' (a: b: a // b) { } (
            map (target: (perTarget target).extraBuildEnv) supportedTargets
          );
        in
        pkgs.mkShell (
          {
            buildInputs = [ rust ] ++ lib.concatMap (target: (perTarget target).buildInputs) supportedTargets;
          }
          // extraEnv
        );
    };
}
