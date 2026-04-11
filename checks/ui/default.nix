{ nixosTest }:
nixosTest {
  name = "ui-test";

  nodes.machine = {
    imports = [ ../common/machine.nix ];

    environment.systemPackages = with pkgs; [
      (python3.withPackages (ps: with ps; [ pexpect ]))
    ];
  };

  testScript = ''
    ${builtins.readFile ../common.py}

    try:
        machine.succeed('${./run-test-in-vm.sh} ${./.} caligula')
    finally: 
        print_logs(machine)
  '';
}
