# Utilities for building, running, and managing caligula development VMs.
{
  self,
  inputs,
  lib,
  ...
}:
{
  perSystem =
    {
      self',
      inputs',
      system,
      pkgs,
      ...
    }:
    let
      /**
        Given a target system, builds a VM runner for that target system.
      */
      makeVMRunner =
        target:
        (inputs.nixpkgs.lib.nixosSystem {
          system = target;
          modules = [
            ./configuration.nix
            (
              { pkgs, ... }:
              {
                # Needed so that the build results can be run by the host machine
                virtualisation.host.pkgs = pkgs;

                # Rename the VM to include the target name
                networking.hostName = "caliguladev-${target}";

                environment.systemPackages =
                  self.devShells.${target}.default.buildInputs
                  ++ (with pkgs; [
                    curl
                    wget
                  ]);
              }
            )
          ];
        }).config.system.build.vm.overrideAttrs
          (_: {
            # Rename the package to something more descriptive
            name = "devvm-${target}";
            pname = "devvm-${target}";

            # Some of pairs require remote compilation, so mark them to be skipped in checks.
            doCheck =
              let
                hostInfo = lib.systems.parse.mkSystemFromString system;
              in
              system == target || hostInfo.kernel.name == "linux";
          });

      supportedLinuxTargets = builtins.filter (
        s: (lib.systems.parse.mkSystemFromString s).kernel.name == "linux"
      ) (self.lib.calculateSupportedTargets system);

      devvms = builtins.map makeVMRunner supportedLinuxTargets;

      usbhotplug = pkgs.writeShellApplication {
        name = "devvm-usbhotplug";
        text = builtins.readFile ./usbhotplug.sh;
      };
    in
    {
      packages = self.lib.packageListToAttrs ([ usbhotplug ] ++ devvms);
    };
}
