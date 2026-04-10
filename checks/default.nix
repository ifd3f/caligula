{
  self,
  lib,
  inputs,
  ...
}:
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

        # There are nix caches for natively-built packages, but not cross-compiled ones.
        # However, caligula also takes forever to build on an emulated platform because Rust(tm).
        # We do this to get the best of both worlds.
        (import inputs.nixpkgs {
          system = "aarch64-linux";
          overlays = [ (final: prev: { caligula = self'.packages.caligula-aarch64-linux; }) ];
        })
      ];

      MiB = 1048576;

      headless = pkgs'.callPackage ./headless { };
      smoke-test-simple = pkgs'.callPackage ./smoke-test-simple { };

      /**
        Overrides the pkgs used by all the VMs in the given test. This is used to make
        cross-platform emulation tests.
      */
      withGuestPkgs =
        guestPkgs: t:
        t.extend {
          modules = [
            (
              { pkgs, lib, ... }:
              {
                name = lib.mkForce "${t.config.name}-${guestPkgs.system}";
                node.pkgs = lib.mkForce guestPkgs;
                qemu.package = pkgs.qemu;
              }
            )
          ];
        };

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
            withGuestPkgs guestPkgs (pkgs'.callPackage ./autoescalate { inherit escalationTool; })
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
            withGuestPkgs guestPkgs (
              pkgs'.callPackage ./blocksize.nix {
                inherit
                  lib
                  blockSize
                  imageSize
                  ;
                diskSizeMiB = 64;
              }
            )
          );

      uiTests = self.lib.cartesianForEach { guestPkgs = guestPkgsOptions; } (
        { guestPkgs }: withGuestPkgs guestPkgs (pkgs.callPackage ./ui { })
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
