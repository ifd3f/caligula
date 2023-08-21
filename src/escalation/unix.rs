use std::borrow::Cow;

use itertools::Itertools;
use shell_words::{join, quote};
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

/// Command components, backed by copy-on-write storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command<'a> {
    pub envs: Vec<(Cow<'a, str>, Cow<'a, str>)>,
    pub proc: Cow<'a, str>,
    pub args: Vec<Cow<'a, str>>,
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

    pub fn wrap_command<'a>(&self, cmd: &Command) -> Command {
        let raw = cmd.to_string();

        match self {
            Self::Sudo => Command {
                envs: vec![],
                proc: "sudo".into(),
                args: vec!["sh".into(), "-c".into(), raw.into()],
            },
            Self::Doas => Command {
                envs: vec![],
                proc: "doas".into(),
                args: vec!["sh".into(), "-c".into(), raw.into()],
            },
            Self::Su => Command {
                envs: vec![],
                proc: "su".into(),
                args: vec![
                    "root".into(),
                    "-c".into(),
                    "sh".into(),
                    "-c".into(),
                    raw.into(),
                ],
            },
        }
    }
}

impl ToString for Command<'_> {
    fn to_string(&self) -> String {
        let args = join([&self.proc].into_iter().chain(self.args.iter()));

        if self.envs.is_empty() {
            args
        } else {
            let envs: String = (self.envs.iter())
                .map(|(k, v)| format!("{}={}", quote(k), quote(v)))
                .join(" ");

            format!("{envs} {args}")
        }
    }
}

impl From<Command<'_>> for std::process::Command {
    fn from(value: Command<'_>) -> Self {
        let mut c = std::process::Command::new(value.proc.as_ref());
        c.args(value.args.iter().map(|a| a.as_ref()));
        c.envs(value.envs.iter().map(|(k, v)| (k.as_ref(), v.as_ref())));
        c
    }
}

impl From<Command<'_>> for tokio::process::Command {
    fn from(value: Command<'_>) -> Self {
        std::process::Command::from(value).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_string_no_env() {
        let command = Command {
            envs: vec![],
            proc: "foo bar".into(),
            args: vec![
                "mrrrrp\\x12 mrp nya nya!".into(),
                "yip yip".into(),
                "yip".into(),
            ],
        };

        let result = command.to_string();

        assert_eq!(result, "'foo bar' 'mrrrrp\\x12 mrp nya nya!' 'yip yip' yip")
    }

    #[test]
    fn test_to_string_with_env() {
        let command = Command {
            envs: vec![("uwu".into(), "nyaaa aaa!".into())],
            proc: "foo bar".into(),
            args: vec![
                "mrrrrp\\x12 mrp nya nya!".into(),
                "yip yip".into(),
                "yip".into(),
            ],
        };

        let result = command.to_string();

        assert_eq!(
            result,
            "uwu='nyaaa aaa!' 'foo bar' 'mrrrrp\\x12 mrp nya nya!' 'yip yip' yip"
        )
    }

    fn get_test_command() -> Command<'static> {
        Command {
            envs: vec![("asdf".into(), "foo".into())],
            proc: "some/proc".into(),
            args: vec!["two".into(), "--three".into(), "\"four\"".into()],
        }
    }

    #[test]
    fn test_sudo() {
        let result = EscalationMethod::Sudo.wrap_command(&get_test_command());

        assert_eq!(
            result.to_string(),
            "sudo sh -c 'asdf=foo some/proc two --three '\\''\"four\"'\\'''"
        )
    }

    #[test]
    fn test_doas() {
        let result = EscalationMethod::Doas.wrap_command(&get_test_command());

        let printed = format!("{result:?}");
        assert_eq!(
            result.to_string(),
            "doas sh -c 'asdf=foo some/proc two --three '\\''\"four\"'\\'''"
        )
    }

    #[test]
    fn test_su() {
        let result = EscalationMethod::Su.wrap_command(&get_test_command());

        let printed = format!("{result:?}");
        assert_eq!(
            result.to_string(),
            "su root -c sh -c 'asdf=foo some/proc two --three '\\''\"four\"'\\'''"
        )
    }
}
