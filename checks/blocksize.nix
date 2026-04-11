{
  lib,
  testers,
  imageSize,
  blockSize,
  diskSizeMiB,
}:
let
  serial = "awawawawawa";
  diskFile = "/tmp/block-file.img";
  byDiskPath = "/dev/disk/by-id/usb-QEMU_QEMU_HARDDISK_${serial}-0:0";
in
testers.runNixOSTest {
  imports = [ ./common/test.nix ];

  name = "blocksize-bs${toString blockSize}-image${toString imageSize}-diskMiB${toString diskSizeMiB}";

  nodes.machine = {
    virtualisation.qemu.options = [
      "-drive if=none,id=usbstick,format=raw,file=${diskFile}"
      "-usb"

      # xhci = USB 3.0. this makes tests go nyoom
      "-device nec-usb-xhci,id=xhci"
      "-device usb-storage,bus=xhci.0,drive=usbstick,serial=${serial},physical_block_size=${toString blockSize}"
    ];
  };

  testScript = with lib; ''
    import os

    print("Creating file image at ${diskFile}")
    os.system("dd bs=1M count=${toString diskSizeMiB} if=/dev/urandom of=${diskFile}")

    ${builtins.readFile ./common/common.py}

    machine.start()
    machine.wait_for_unit('default.target')
    print(machine.execute('stat $(readlink -f ${byDiskPath})', check_output=True)[1])
    try:
        machine.succeed('dd if=/dev/urandom of=/tmp/input.iso bs=1 count=${toString imageSize}')
        with subtest("executes successfully"):
            machine.succeed('caligula burn /tmp/input.iso --force -o $(readlink -f ${byDiskPath}) --hash skip --compression auto --interactive never')

        with subtest("burns correctly"):
            machine.succeed('dd if=${byDiskPath} of=/tmp/written.iso bs=1 count=${toString imageSize}')
            machine.succeed('diff -s /tmp/input.iso /tmp/written.iso')
        
    finally: 
        print_logs(machine)
  '';
}
