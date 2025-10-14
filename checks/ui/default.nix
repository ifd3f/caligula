{ nixosTest, caligula }:
nixosTest {
  name = "ui-test";

  nodes.machine = { pkgs, lib, ... }:
    with lib; {
      security.sudo = {
        enable = true;
        wheelNeedsPassword = false;
      };

      users.users = {
        admin = {
          isNormalUser = true;
          extraGroups = [ "wheel" ];
        };
      };

      environment.systemPackages = with pkgs; [
        caligula
        (python3.withPackages (ps: with ps; [ pexpect ]))
      ];
    };

  testScript = ''
    ${builtins.readFile ../common.py}

    try:
        machine.succeed('${./run-test-in-vm.sh} ${./.} ${caligula}/bin/caligula')
    finally: 
        print_logs(machine)
  '';
}
