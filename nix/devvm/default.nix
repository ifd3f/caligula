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
      makeVMRunner =
        target:
        # Make a NixOS VM
        (inputs.nixpkgs.lib.nixosSystem {
          system = target;
          modules = [
            ./configuration.nix
            {
              # Needed so that the build results can be run by the host machine
              virtualisation.host.pkgs = pkgs;

              # Rename the VM to include the target name
              networking.hostName = "caliguladev-${target}";

              environment.systemPackages = self.devShells.${target}.default.buildInputs;
            }
          ];
        }).config.system.build.vm.overrideAttrs
          (_: {
            # Rename the package to something more descriptive
            name = "devvm-${target}";
          });
    in
    {
      packages.devvm-aarch64-linux = makeVMRunner "aarch64-linux";
      packages.devvm-x86_64-linux = makeVMRunner "x86_64-linux";
      packages.devvm-usbhotplug = with pkgs; writeShellApplication {
        name = "devvm-usbhotplug";
        text = builtins.readFile ./usbhotplug.sh;
      };
    };
}
