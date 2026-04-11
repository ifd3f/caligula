{ nixosTest, escalationTool }:
nixosTest {
  name = "autoescalate-${escalationTool}";

  nodes.machine = {
    imports = [ ../common/machine.nix ];
    config.caligula.escalationTool = escalationTool;
  };

  testScript = ''
    ${builtins.readFile ../common/common.py}

    try:
        # Set up loop devices
        machine.succeed('dd if=/dev/zero of=/tmp/blockfile bs=1M count=1')
        machine.succeed('dd if=/dev/urandom of=/tmp/input.iso bs=100K count=1')
        machine.succeed('losetup /dev/loop0 /tmp/blockfile')

        # Sanity check: can we run something without asking for a password?
        machine.succeed('timeout 10 su admin -c "${escalationTool} -- echo We are able to escalate without asking for a password"')

        with subtest("should succeed when run as non-root wheel user"):
            machine.succeed('timeout 10 su admin -c "caligula burn /tmp/input.iso --force -o /dev/loop0 --hash skip --compression auto --root always --interactive never"')
    finally: 
        print_logs(machine)
  '';
}
