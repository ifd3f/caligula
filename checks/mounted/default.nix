{ testers, pkgs }:
testers.runNixOSTest {
  imports = [ ../common/test.nix ];
  name = "mounted-targets";

  nodes.machine.environment.systemPackages = with pkgs; [
    (python3.withPackages (ps: [ ps.pexpect ]))
    util-linux
    e2fsprogs
  ];

  testScript = ''
    ${builtins.readFile ../common/common.py}

    try:
        machine.succeed("unshare --mount --propagation private env CALIGULA=caligula python3 ${./test_mounted.py}")
    finally:
        print_logs(machine)
  '';
}
