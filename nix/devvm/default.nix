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
      hostPkgs = pkgs;

      /**
        Given a target system, builds a VM runner for that target system.
      */
      makeVMRunner =
        target:
        let
          extraModule =
            { pkgs, ... }:
            {
              # Needed so that the build results can be run by the host machine
              virtualisation.host.pkgs = hostPkgs;

              # Rename the VM to include the target name
              networking.hostName = "caliguladev-${target}";

              environment.systemPackages =
                self.devShells.${target}.default.buildInputs
                ++ (with pkgs; [
                  curl
                  tmux
                  wget
                ]);
            };

          nixos = inputs.nixpkgs.lib.nixosSystem {
            system = target;
            modules = [
              ./configuration.nix
              extraModule
            ];
          };

          # Because I'm doing all sorts of deranged garbage to the existing VM script
          wrapper =
            with pkgs;
            writeShellApplication {
              name = "devvm-${target}";
              runtimeInputs = [ coreutils ];
              text = ''
                CALIGULA_DIR="$(readlink -f .)" exec ${nixos.config.system.build.vm}/bin/run-caliguladev-${target}-vm
              '';
            };
        in
        wrapper.overrideAttrs (_: {
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
