use crate::{
    store::{Host, Store},
    vault::DesktopVault,
};
use anyhow::{Context, Result, bail};
use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::process::Command;
use zeroize::Zeroizing;

pub fn arguments(host: &Host) -> Vec<String> {
    let mut args = [
        "-tt",
        "-o",
        "ControlPath=none",
        "-o",
        "ForkAfterAuthentication=no",
    ]
    .map(String::from)
    .to_vec();
    if let Some(port) = host.port {
        args.extend(["-p".into(), port.to_string()]);
    }
    if !host.username.is_empty() {
        args.extend(["-l".into(), host.username.clone()]);
    }
    if host.secret.is_some() {
        args.extend(
            [
                "-o",
                "PreferredAuthentications=password",
                "-o",
                "PasswordAuthentication=yes",
                "-o",
                "NumberOfPasswordPrompts=1",
            ]
            .map(String::from),
        );
    } else if !host.key.is_empty() {
        args.extend([
            "-i".into(),
            host.key.clone(),
            "-o".into(),
            "IdentitiesOnly=yes".into(),
        ]);
    }
    args.extend(["--".into(), host.hostname.clone()]);
    args
}

pub fn run_session(id: &str) -> Result<()> {
    let host = Store::desktop()?.get(id)?;
    let mut ssh = Command::new("ssh");
    ssh.args(arguments(&host));
    if let Some(secret) = &host.secret {
        ssh.env("SSH_ASKPASS", env::current_exe()?)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("SSH_MANAGER_ASKPASS", "1")
            .env("SSH_MANAGER_CREDENTIAL", secret)
            .env("SSH_MANAGER_SSH_PID", std::process::id().to_string())
            .env_remove("SSH_ASKPASS_PROMPT");
    }
    Err(ssh.exec()).context("Could not start the OpenSSH client")
}

fn vault_prompt(text: &str, hint: &str, parent: u32, expected: Option<u32>) -> bool {
    let account = text
        .trim_end_matches([' ', '\t'])
        .strip_suffix("'s password:")
        .unwrap_or("");
    hint.is_empty()
        && expected == Some(parent)
        && !account.chars().any(char::is_whitespace)
        && account
            .rsplit_once('@')
            .is_some_and(|(user, host)| !user.is_empty() && !host.is_empty())
}

pub fn askpass(prompt: &str) -> Result<()> {
    let hint = env::var("SSH_ASKPASS_PROMPT").unwrap_or_default();
    let expected = env::var("SSH_MANAGER_SSH_PID")
        .ok()
        .and_then(|s| s.parse().ok());
    // getppid has no preconditions. Only the original destination ssh may use the vault.
    let parent = unsafe { libc::getppid() } as u32;
    let answer = if vault_prompt(prompt, &hint, parent, expected) {
        DesktopVault.get(&env::var("SSH_MANAGER_CREDENTIAL")?)?
    } else {
        let mut output = OpenOptions::new().write(true).open("/dev/tty")?;
        if hint == "none" {
            writeln!(output, "{prompt}")?;
            return Ok(());
        }
        if hint == "confirm" || prompt.contains("yes/no") || prompt.contains("'yes' or 'no'") {
            write!(output, "{prompt}")?;
            output.flush()?;
            let mut answer = String::new();
            if BufReader::new(File::open("/dev/tty")?).read_line(&mut answer)? == 0 {
                bail!("Prompt cancelled.");
            }
            Zeroizing::new(answer.trim_end_matches(['\r', '\n']).to_owned())
        } else {
            Zeroizing::new(rpassword::prompt_password(prompt)?)
        }
    };
    // stdout is OpenSSH's private pipe. The password never goes to the terminal.
    writeln!(io::stdout().lock(), "{}", answer.as_str())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restrict_vault_to_password_and_destination_process() {
        let prompt = "me@[::1]:2222's password: ";
        assert!(vault_prompt(prompt, "", 5, Some(5)));
        assert!(!vault_prompt(prompt, "", 8, Some(5)));
        for (p, h) in [
            (prompt, "confirm"),
            ("Verification code: ", ""),
            ("yes/no", ""),
            ("banner\nme@host's password: ", ""),
            ("Enter passphrase for key: ", ""),
        ] {
            assert!(!vault_prompt(p, h, 5, Some(5)));
        }
    }
}
