{ self, nixpkgs, ... }:
system:
let
  pkgs = import nixpkgs {
    inherit system;
    overlays = [ self.overlays.default ];
  };
in {
  headless = pkgs.callPackage ./headless { };
  smoke-test-simple = pkgs.callPackage ./smoke-test-simple { };
}
