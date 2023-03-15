{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
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

          lint-script =
            let path = lib.makeBinPath [ crossHelpers.baseToolchain ];
            in pkgs.writeScriptBin "lint" ''
              export PATH=${path}
              ${./scripts/lint.sh}
            '';

          caligula = self.packages."${system}"."caligula-${system}";
        } // crossHelpers.caligulaPackages;

        devShells.default = crossHelpers.crossCompileDevShell.overrideAttrs
          (final: prev: {
            buildInputs = prev.buildInputs ++ (with pkgs; [ nixfmt ]);
          });
      }));
}
