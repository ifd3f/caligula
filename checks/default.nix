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

      # Our VM tests vary over guestPkgs to allow us to do Funny Business
      # (testing cross-compiled executables in emulated VMs)
      guestPkgsOptions = [
        pkgs'
        pkgs'.pkgsCross.aarch64-multiplatform
      ];

      MiB = 1048576;

      headless = pkgs'.callPackage ./headless { };
      smoke-test-simple = pkgs'.callPackage ./smoke-test-simple { };

      autoescalateTests =
        self.lib.cartesianForEach
          {
            escalationTool = [
              "doas"
              "sudo"
              "run0"
            ];
            guestPkgs = guestPkgsOptions;
          }
          (
            { escalationTool, guestPkgs }:
            pkgs'.callPackage ./autoescalate { inherit guestPkgs escalationTool; }
          );

      blocksizeTests =
        self.lib.cartesianForEach
          {
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
            guestPkgs = guestPkgsOptions;
          }
          (
            {
              imageSize,
              blockSize,
              guestPkgs,
            }:
            pkgs'.callPackage ./blocksize.nix {
              inherit
                lib
                blockSize
                imageSize
                guestPkgs
                ;
              diskSizeMiB = 64;
            }
          );

      uiTests = self.lib.cartesianForEach { guestPkgs = guestPkgsOptions; } (
        {guestPkgs}: pkgs.callPackage ./ui { inherit guestPkgs; }
      );
    in
    {
      checks = self.lib.packageListToAttrs (
        [
          headless
          smoke-test-simple
        ]
        ++ lib.optionals (system == "x86_64-linux") (autoescalateTests ++ blocksizeTests ++ uiTests)
      );
    };
}
