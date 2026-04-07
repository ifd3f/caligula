# Helpers for building and cross-compiling Caligula.
{
  lib,
  ...
}:
{
  perSystem =
    {
      self',
      pkgs,
      system,
      ...
    }:
    {
      packages.caligula-bin-aur = pkgs.callPackage ./caligula-bin-aur.nix {
        caligula = self'.packages.caligula;
      };
    };
}
