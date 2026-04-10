{ lib, config, ... }:
{
  options.lib = lib.mkOption {
    type = lib.types.attrsOf lib.types.anything;
    default = { };
    description = "Shared library functions";
  };

  config = {
    flake.lib = config.lib;

    lib = {
      /**
        Turn a list of packages into an attrset mapping pname -> package.
      */
      packageListToAttrs =
        pkgsList:
        builtins.listToAttrs (
          map (p: lib.nameValuePair (if p ? pname then p.pname else p.name) p) pkgsList
        );

      /**
        Calculate what a given host system is able to cross-compile for.
      */
      calculateSupportedTargets =
        system:
        let
          hostInfo = lib.systems.parse.mkSystemFromString system;
          targets = [
            system # Systems are (generally) able to build themselves.
          ]
          ++ lib.optionals (hostInfo.kernel.name == "linux") [
            "aarch64-linux"
            "x86_64-linux"
          ]
          ++ lib.optionals (system == "aarch64-darwin") [
            "aarch64-darwin"
            "aarch64-linux"
          ];
          # Notes on what's missing:
          # - x86_64-darwin -> aarch64-darwin doesn't seem supported.
        in
        # Sort and uniquify the list of systems.
        lib.lists.sort (a: b: a < b) (lib.lists.uniqueStrings targets);

      /**
        Given xss and f, equivalent to map f (lib.cartesianProduct xss)
      */
      cartesianForEach = xss: f: builtins.map f (lib.cartesianProduct xss);
    };
  };
}
