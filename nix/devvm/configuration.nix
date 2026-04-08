{
  config,
  pkgs,
  lib,
  modulesPath,
  ...
}:
{
  imports = [
    "${modulesPath}/profiles/minimal.nix"
    "${modulesPath}/profiles/qemu-guest.nix"

    # Needed so that the QEMU options exist
    "${modulesPath}/virtualisation/qemu-vm.nix"
  ];

  networking.useDHCP = true;

  # We don't need a bootloader because qemu goes straight to kernel + initrd
  boot.loader.grub.enable = false;

  # Automatically log in as development user
  services.getty.autologinUser = "incitatus";
  users.users.incitatus = {
    isNormalUser = true;
    password = "";
    extraGroups = [ "wheel" ];
  };

  # Ensure root doesn't need a password
  security.sudo.wheelNeedsPassword = false;
  users.users.root.password = "";

  system.stateVersion = "25.11";

  virtualisation = {
    mountHostNixStore = true;
    useBootLoader = false;

    qemu.options = ["-cpu" "host"];
    cores = 8;
    memorySize = 2048; # MiB

    # Don't make a disk image. The VM should run off a tmpfs.
    diskImage = null;

    # Make it run directly in the console
    graphics = false;
  };
}
