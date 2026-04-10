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

    qemu.options = [
      # Expose host's CPU to guest as normal
      "-cpu host"

      # Expose VM's monitor console to a socket
      "-monitor unix:/tmp/caligula-devvm-monitor.sock,server,nowait"

      # Needed or else ctrl-c kills the VM
      "-serial mon:stdio"

      # Create a USB bus named xhci. We will be sticking devices
      # onto this for testing purposes.
      "-device nec-usb-xhci,id=xhci"
    ];
    cores = 8;
    memorySize = 2048; # MiB

    # Don't make a disk image. The VM should run off a tmpfs.
    diskImage = null;

    # Make it run directly in the console
    graphics = false;
  };
}
