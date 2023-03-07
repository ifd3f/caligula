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

        crossHelpers = self.lib.crossHelpers system;
      in {
        packages = {
          default = self.packages."${system}".caligula;
          caligula = self.packages."${system}"."caligula-${system}";
        } // crossHelpers.caligulaPackages;

        devShell = let tc = crossHelpers.forTarget system;
        in with pkgs;
        mkShell {
          buildInputs = [ nixfmt tc.rust-toolchain-dev ] ++ tc.platformDeps;
          CARGO_BUILD_TARGET = tc.rustTarget;
        };
      }));
}
