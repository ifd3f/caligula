{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, naersk, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        lib = pkgs.lib;

        # On Linux, we want to only support one target.
        sysinfo = lib.systems.parse.mkSystemFromString system;
        rustTarget = if sysinfo.kernel.name == "linux" then
          "${sysinfo.cpu.name}-unknown-linux-musl"
        else if sysinfo.kernel.name == "darwin" then
          "${sysinfo.cpu.name}-apple-darwin"
        else
          throw "unknown system ${system}";

        rust-toolchain = (pkgs.rust-bin.stable.latest.default.override {
          targets = [ rustTarget ];
        });
        rust-toolchain-dev = rust-toolchain.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
        naersk' = pkgs.callPackage naersk {
          cargo = rust-toolchain;
          rustc = rust-toolchain;
        };

      in {
        packages.default = with pkgs;
          naersk'.buildPackage {
            src = "${self}";
            doCheck = true;
            CARGO_BUILD_TARGET = rustTarget;
          };

        devShell = with pkgs;
          mkShell {
            buildInputs = [ rust-toolchain-dev ];
            CARGO_BUILD_TARGET = rustTarget;
          };
      });
}
