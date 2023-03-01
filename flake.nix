{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, utils, naersk, rust-overlay }:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        naersk-lib = pkgs.callPackage naersk { };
        rust-toolchain =
          pkgs.rust-bin.selectLatestNightlyWith (toolchain: toolchain.default);
        rust-toolchain-dev = rust-toolchain.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
      in {
        packages.default = naersk-lib.buildPackage ./.;

        devShell = with pkgs;
          mkShell {
            buildInputs = [ openssl pkg-config rust-toolchain-dev ];
          };
      });
}
