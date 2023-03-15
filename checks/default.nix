{ self, nixpkgs, ... }:
system:
let
  pkgs = import nixpkgs {
    inherit system;
    overlays = [ self.overlays.default ];
  };
in {
  smoke-test-simple = pkgs.callPackage ./smoke-test-simple { };
}
