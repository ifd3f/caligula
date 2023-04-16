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
} //

(if system == "x86_64-linux" then {
  autoescalate-doas =
    pkgs.callPackage ./autoescalate { escalationTool = "doas"; };
  autoescalate-sudo =
    pkgs.callPackage ./autoescalate { escalationTool = "sudo"; };
} else
  { })
