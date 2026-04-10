{ self, lib, ... }:
{
  perSystem =
    {
      self',
      system,
      pkgs,
      ...
    }:
    let
      # need to put caligula into the pkgs instance
      pkgs' = pkgs.extend (self.overlays.default);

      headless = pkgs'.callPackage ./headless { };
      smoke-test-simple = pkgs'.callPackage ./smoke-test-simple { };

      autoescalateTests =
        map (escalationTool: pkgs'.callPackage ./autoescalate { inherit escalationTool; })
          [
            "doas"
            "sudo"
            "run0"
          ];

      blocksizeTests =
        let
          MiB = 1048576;
          parameters = lib.cartesianProduct {
            blockSize = [
              512
              1024
              2048
              4096
              8192
            ];
            imageSize = [
              (10 * MiB)
              (10 * MiB + 51)
            ];
          };
        in
        map (
          { imageSize, blockSize }:
          pkgs'.callPackage ./blocksize.nix {
            inherit lib blockSize imageSize;
            diskSizeMiB = 64;
          }
        ) parameters;

    in
    {
      checks = self.lib.packageListToAttrs (
        [
          headless
          smoke-test-simple
        ]
        ++ lib.optionals (system == "x86_64-linux") (autoescalateTests ++ blocksizeTests)
      );
    };
}
