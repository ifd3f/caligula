use std::process::Command;
use which::which;

use super::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display)]
pub enum EscalationMethod {
    #[display(fmt = "sudo")]
    Sudo,
    #[display(fmt = "doas")]
    Doas,
    #[display(fmt = "su")]
    Su,
}

impl EscalationMethod {
    const ALL: [EscalationMethod; 3] = [Self::Sudo, Self::Doas, Self::Su];

    pub fn detect() -> Result<Self, Error> {
        for m in Self::ALL {
            if m.is_supported() {
                return Ok(m);
            }
        }
        Err(Error::UnixNotDetected)
    }

    fn is_supported(&self) -> bool {
        which(self.cmd_name()).is_ok()
    }

    fn cmd_name(&self) -> &str {
        match self {
            Self::Sudo => "sudo",
            Self::Doas => "doas",
            Self::Su => "su",
        }
    }

    pub fn wrap_command<'a>(&self, cmd: Command) -> Command {
        let raw = format!("{cmd:?}");
        match self {
            Self::Sudo => {
                let mut cmd = Command::new("sudo");
                cmd.args(["sh", "-c", &raw]);
                cmd
            }
            Self::Doas => {
                let mut cmd = Command::new("doas");
                cmd.args(["sh", "-c", &raw]);
                cmd
            }
            Self::Su => {
                let mut cmd = Command::new("su");
                cmd.args(["root", "sh", "-c", &raw]);
                cmd
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::EscalationMethod;

    fn get_test_command() -> Command {
        let mut cmd = Command::new("some/proc");
        cmd.arg("two")
            .arg("--three")
            .arg("\"four\"")
            .env("asdf", "foo");
        cmd
    }

    #[test]
    fn test_sudo() {
        let result = EscalationMethod::Sudo.wrap_command(get_test_command());

        let printed = format!("{result:?}");
        assert_eq!(
            printed,
            r#""sudo" "sh" "-c" "\"some/proc\" \"two\" \"--three\" \"\\\"four\\\"\"""#
        )
    }

    #[test]
    fn test_doas() {
        let result = EscalationMethod::Doas.wrap_command(get_test_command());

        let printed = format!("{result:?}");
        assert_eq!(
            printed,
            r#""doas" "sh" "-c" "\"some/proc\" \"two\" \"--three\" \"\\\"four\\\"\"""#
        )
    }

    #[test]
    fn test_su() {
        let result = EscalationMethod::Su.wrap_command(get_test_command());

        let printed = format!("{result:?}");
        assert_eq!(
            printed,
            r#""su" "root" "sh" "-c" "\"some/proc\" \"two\" \"--three\" \"\\\"four\\\"\"""#
        )
    }
}
