{ self, nixpkgs, ... }:
system:
let
  pkgs = import nixpkgs {
    inherit system;
    overlays = [ self.overlays.default ];
  };
  lib = pkgs.lib;
in with lib;
{
  headless = pkgs.callPackage ./headless { };
  smoke-test-simple = pkgs.callPackage ./smoke-test-simple { };
} //

(if system == "x86_64-linux" then
  {
    autoescalate-doas =
      pkgs.callPackage ./autoescalate { escalationTool = "doas"; };
    autoescalate-sudo =
      pkgs.callPackage ./autoescalate { escalationTool = "sudo"; };
     autoescalate-run0 =
       pkgs.callPackage ./autoescalate { escalationTool = "run0"; };
  } //

  # blocksize alignment tests
  (let
    MiB = 1048576;
    parameters = cartesianProduct {
      blockSize = [ 512 1024 2048 4096 8192 ];
      imageSize = [ (10 * MiB) (10 * MiB + 51) ];
    };
  in listToAttrs (map ({ imageSize, blockSize }: rec {
    name = value.name;
    value = pkgs.callPackage ./blocksize.nix {
      inherit lib blockSize imageSize;
      diskSizeMiB = 64;
    };
  }) parameters))
else
  { })
