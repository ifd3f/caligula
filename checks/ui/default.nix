{ testers }:
testers.runNixOSTest {
  imports = [ ../common/test.nix ];
  name = "ui-test";

  nodes.machine = {
    environment.systemPackages = with pkgs; [
      (python3.withPackages (ps: with ps; [ pexpect ]))
    ];
  };

  testScript = ''
    ${builtins.readFile ../common/ommon.py}

    try:
        machine.succeed('${./run-test-in-vm.sh} ${./.} caligula')
    finally: 
        print_logs(machine)
  '';
}
