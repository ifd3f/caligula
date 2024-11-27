{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    flake-utils.url = "github:numtide/flake-utils";

    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, naersk, rust-overlay }@inputs:
    {
      lib = import ./nix inputs;
      overlays.default = final: prev: {
        caligula = self.packages.${prev.system}.caligula;
      };
    } //

    (let
      supportedSystems =
        [ "aarch64-darwin" "aarch64-linux" "x86_64-darwin" "x86_64-linux" ];
    in flake-utils.lib.eachSystem supportedSystems (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        lib = pkgs.lib;

        crossHelpers = self.lib.crossHelpers system;
      in {
        checks = import ./checks inputs system;

        packages = {
          default = self.packages."${system}".caligula;

          lint-script = pkgs.writeScriptBin "lint.sh" ''
            #!/bin/sh
            export PATH=${lib.makeBinPath [ crossHelpers.baseToolchain ]}
            ${./scripts/lint.sh}
          '';

          caligula = self.packages."${system}"."caligula-${system}";
        }

          // crossHelpers.caligulaPackages

          // (if system == "x86_64-linux" then {
            caligula-bin-aur = pkgs.callPackage ./nix/aur.nix {
              caligula = self.packages.x86_64-linux.caligula;
            };
          } else
            { });

        devShells.default = crossHelpers.crossCompileDevShell.overrideAttrs
          (final: prev: {
            buildInputs = prev.buildInputs ++ (with pkgs; [ nixfmt ]);
          });
      }));
}
