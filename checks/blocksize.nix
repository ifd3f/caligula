{ lib, nixosTest, imageSize, blockSize, diskSizeMiB }:
let
  serial = "awawawawawa";
  diskFile = "/tmp/block-file.img";
  byDiskPath = "/dev/disk/by-id/usb-QEMU_QEMU_HARDDISK_${serial}-0:0";
in nixosTest {
  name = "blocksize-bs${toString blockSize}-image${toString imageSize}-diskMiB${
      toString diskSizeMiB
    }";

  nodes.machine = { pkgs, lib, ... }:
    with lib; {
      imports = [ ];

      users.users = {
        admin = {
          isNormalUser = true;
          extraGroups = [ "wheel" ];
        };
      };

      environment.systemPackages = with pkgs; [ caligula ];
      virtualisation.qemu.options =
        [ "-drive" "if=none,id=usbstick,format=raw,file=${diskFile}" ]
        ++ [ "-usb" ] ++ [ "-device" "usb-ehci,id=ehci" ] ++ [
          "-device"
          "usb-storage,bus=ehci.0,drive=usbstick,serial=${serial},physical_block_size=${
            toString blockSize
          }"
        ];
    };

  testScript = with lib; ''
    import os

    print("Creating file image at ${diskFile}")
    os.system("dd bs=1M count=${
      toString diskSizeMiB
    } if=/dev/urandom of=${diskFile}")

    ${readFile ./common.py}

    machine.start()
    machine.wait_for_unit('default.target')
    print(machine.execute('stat $(readlink -f ${byDiskPath})', check_output=True)[1])
    try:
        machine.succeed('dd if=/dev/urandom of=/tmp/input.iso bs=1 count=${
          toString imageSize
        }')
        with subtest("executes successfully"):
            machine.succeed('caligula burn /tmp/input.iso --force -o $(readlink -f ${byDiskPath}) --hash skip --compression auto --interactive never')

        with subtest("burns correctly"):
            machine.succeed('dd if=${byDiskPath} of=/tmp/written.iso bs=1 count=${
              toString imageSize
            }')
            machine.succeed('diff -s /tmp/input.iso /tmp/written.iso')
        
    finally: 
        print_logs(machine)
  '';
}
