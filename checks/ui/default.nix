{ testers }:
testers.runNixOSTest {
  imports = [ ../common/test.nix ];
  name = "ui-test";

  nodes.machine =
    { pkgs, ... }:
    {
      environment.systemPackages = with pkgs; [
        (python3.withPackages (ps: with ps; [ pexpect ]))
      ];
    };

  testScript = ''
    ${builtins.readFile ../common/common.py}

    try:
        machine.succeed('${./run-test-in-vm.sh} ${./.} caligula')
    finally: 
        print_logs(machine)
  '';
}
