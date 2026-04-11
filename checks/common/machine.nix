/*
  Common NixOS module usable by all NixOS tests that run caligula.

  Preinstalls caligula (of course), and sets up users and common escalation
  environment items.
*/

{
  pkgs,
  lib,
  config,
  ...
}:
with lib;
let
  cfg = config.caligula;
in
{
  options.caligula = {
    adminUser.enable = mkOption {
      default = true;
      description = "Whether to enable the admin user on the machine.";
      type = types.bool;
    };

    escalationTool = mkOption {
      default = "sudo";
      description = "Which escalation tool to install, or null if none at all.";
      type = types.enum [
        "sudo"
        "doas"
        "run0"
        null
      ];
    };
  };

  config = mkMerge [
    {
      environment.systemPackages = with pkgs; [ caligula ];

      users.users.admin = mkIf cfg.adminUser.enable {
        isNormalUser = true;
        extraGroups = [ "wheel" ];
      };

      networking = {
        # firewall makes vm setup slowwwwww
        firewall.enable = false;

        # dhcp also makes vm setup slowwwww so just statically configure that shit
        dhcpcd.enable = false;
        interfaces.eth0.ipv4.addresses = [
          {
            address = "10.0.2.15";
            prefixLength = 24;
          }
        ];
        defaultGateway = {
          address = "10.0.2.1";
          interface = "eth0";
        };
      };
    }

    (mkIf (cfg.escalationTool == null) { security.sudo.enable = mkForce false; })
    (mkIf (cfg.escalationTool == "sudo") {
      security.sudo = {
        enable = true;
        wheelNeedsPassword = false;
      };
    })

    (mkIf (cfg.escalationTool == "doas") {
      security.sudo.enable = mkForce false;
      security.doas = {
        enable = true;
        wheelNeedsPassword = false;
      };
    })

    (mkIf (cfg.escalationTool == "run0") {
      security.sudo.enable = mkForce false;

      security.polkit.enable = true;

      # see https://warlord0blog.wordpress.com/2024/07/30/passwordless-run0/
      security.polkit.extraConfig = ''
        polkit.addRule(function(action, subject) {
            if (action.id == "org.freedesktop.systemd1.manage-units") {
                if (subject.isInGroup("wheel")) {
                    return polkit.Result.YES;
                }
            }
        });
      '';

      security.pam.services.su.requireWheel = true;
    })
  ];
}
