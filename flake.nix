{
  description = "Caligula flake";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";

    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs@{ flake-parts, rust-overlay, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      debug = true;
      imports = [
        ./nix/build-helpers
        ./nix/aur
        ./nix/devvm
        ./checks
      ];
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];

      flake = {
        lib.naersk = inputs.naersk;
        overlays = {
          # User-facing overlay
          default = final: prev: {
            caligula = inputs.self.packages.${prev.system}.caligula;
          };

          # Re-export of rust-overlay for internal use
          rust-overlay = inputs.rust-overlay.overlays.default;
        };
      };

      perSystem =
        {
          config,
          self',
          pkgs,
          system,
          ...
        }:
        {

          # Instantiate a very basic and standard pkgs.
          # Modules that need to tweak it must instantiate their own.
          _module.args.pkgs = import inputs.nixpkgs {
            inherit system;
          };

          packages.default = self'.packages.caligula;

          devShells.default = self'.devShells.cross.overrideAttrs (
            final: prev: {
              buildInputs =
                prev.buildInputs
                ++ (with pkgs; [
                  nixfmt
                  python3
                ]);
            }
          );
        };
    };
}
