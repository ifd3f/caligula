# Helpers for building and cross-compiling Caligula.
{
  self,
  inputs,
  lib,
  ...
}:
{
  perSystem =
    { self', system, ... }:
    let
      # Build new pkgs instance with rust overlay. Don't use the global pkgs.
      pkgs = import inputs.nixpkgs {
        inherit system;
        overlays = [ self.overlays._rust-overlay ];
      };

      supportedTargets = self.lib.calculateSupportedTargets system;
      baseToolchain = pkgs.rust-bin.stable.latest.default;

      perTarget =
        target:
        pkgs.callPackage ./cross-helpers.nix {
          inherit target baseToolchain;
          inherit (inputs) naersk;
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
