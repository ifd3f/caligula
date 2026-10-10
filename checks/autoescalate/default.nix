{ testers, escalationTool }:
testers.runNixOSTest {
  imports = [ ../common/test.nix ];

  name = "autoescalate-${escalationTool}";

  nodes.machine = {
    config.caligula.escalationTool = escalationTool;
  };

  testScript = ''
    ${builtins.readFile ../common/common.py}

    try:
        # Set up loop devices
        machine.succeed('dd if=/dev/zero of=/tmp/blockfile bs=1M count=32')
        machine.succeed('dd if=/dev/urandom of=/tmp/input.iso bs=100K count=1')
        machine.succeed('losetup /dev/loop0 /tmp/blockfile')

        # Sanity check: can we run something without asking for a password?
        machine.succeed('timeout 10 su admin -c "${escalationTool} -- echo We are able to escalate without asking for a password"')

        with subtest("should succeed when run as non-root wheel user"):
            machine.succeed('timeout 10 su admin -c "caligula burn /tmp/input.iso --force -o /dev/loop0 --hash skip --compression auto --root always --interactive never"')

        with subtest("should escalate when unmounting requires root"):
            machine.succeed('mkfs.ext4 -F /dev/loop0')
            machine.succeed('mkdir /mnt/target && mount /dev/loop0 /mnt/target')
            machine.succeed('timeout 10 su admin -c "caligula burn /tmp/input.iso --force -o /dev/loop0 --hash skip --compression auto --root always --interactive never"')
            machine.fail('mountpoint -q /mnt/target')
            machine.succeed('cmp -n 102400 /tmp/input.iso /dev/loop0')
    finally: 
        print_logs(machine)
  '';
}
