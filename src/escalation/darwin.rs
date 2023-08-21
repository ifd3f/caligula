use super::unix::{Command, EscalationMethod};

pub async fn wrap_osascript_escalation(raw: Command<'_>) -> anyhow::Result<tokio::process::Child> {
    for _ in 0..3 {
        // User-friendly thing that lets you use touch ID if you wanted.
        // https://apple.stackexchange.com/questions/23494/what-option-should-i-give-the-sudo-command-to-have-the-password-asked-through-a
        // We loop because your finger might not be recognized sometimes.

        let result = tokio::process::Command::new("osascript")
            .arg("-e")
            .arg("do shell script \"mkdir -p /var/db/sudo/$USER; touch /var/db/sudo/$USER\" with administrator privileges")
            .kill_on_drop(true)
            .spawn()?
            .wait()
            .await?;

        if result.success() {
            break;
        }
    }

    let cmd: tokio::process::Command = EscalationMethod::Sudo.wrap_command(raw).into();
    Ok(cmd.kill_on_drop(true).spawn()?)
}
