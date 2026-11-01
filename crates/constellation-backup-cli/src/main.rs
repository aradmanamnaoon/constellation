use std::{
    env,
    io::{self, IsTerminal},
    path::PathBuf,
    process::ExitCode,
};

use constellation_backup_cli::{max_limits, BackupService};
use zeroize::Zeroizing;

const USAGE: &str = "Constellation offline encrypted SQLite backup/restore CLI

Usage:
  constellation-backup-cli backup <source.sqlite3> <new-backup.cbackup> [--max-mib 1..64]
  constellation-backup-cli restore <backup.cbackup> <new-database.sqlite3> [--max-mib 1..64]
  constellation-backup-cli --help

Passphrases are entered on a real interactive terminal without echo.
Passing a passphrase through command-line arguments, environment variables,
shell pipelines or redirected standard input is NOT supported.
Only new output paths are accepted; existing files are never overwritten.
The default maximum unencrypted SQLite image is 16 MiB (absolute cap: 64 MiB).
";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    Backup {
        source: PathBuf,
        target: PathBuf,
        max_mib: u8,
    },
    Restore {
        source: PathBuf,
        target: PathBuf,
        max_mib: u8,
    },
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, &'static str> {
    let args: Vec<String> = args.into_iter().collect();
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h" | "help") {
        return Ok(Command::Help);
    }
    if !(args.len() == 3 || args.len() == 5) {
        return Err("expected a command, source and new destination");
    }
    let max_mib = if args.len() == 5 {
        if args[3] != "--max-mib" {
            return Err("only --max-mib is an accepted option; passphrases must not be provided as arguments");
        }
        let number = args[4]
            .parse::<u8>()
            .map_err(|_| "invalid --max-mib value")?;
        if !(1..=64).contains(&number) {
            return Err("--max-mib must be between 1 and 64");
        }
        number
    } else {
        16
    };
    if args[1].starts_with('-') || args[2].starts_with('-') {
        return Err("source and destination paths must not be command options");
    }
    match args[0].as_str() {
        "backup" => Ok(Command::Backup {
            source: PathBuf::from(&args[1]),
            target: PathBuf::from(&args[2]),
            max_mib,
        }),
        "restore" => Ok(Command::Restore {
            source: PathBuf::from(&args[1]),
            target: PathBuf::from(&args[2]),
            max_mib,
        }),
        _ => Err("unknown command"),
    }
}

fn password_prompt(prompt: &str) -> Result<Zeroizing<String>, &'static str> {
    let raw =
        rpassword::prompt_password(prompt).map_err(|_| "secure terminal password input failed")?;
    Ok(Zeroizing::new(raw))
}

fn run() -> Result<(), String> {
    let command = parse(env::args().skip(1)).map_err(|error| error.to_string())?;
    if command == Command::Help {
        println!("{USAGE}");
        return Ok(());
    }
    let (source, target, max_mib, creating_backup) = match command {
        Command::Backup {
            source,
            target,
            max_mib,
        } => (source, target, max_mib, true),
        Command::Restore {
            source,
            target,
            max_mib,
        } => (source, target, max_mib, false),
        Command::Help => unreachable!(),
    };
    // Reject common errors before prompting and before Argon2 costs.
    if !std::fs::symlink_metadata(&source).is_ok_and(|metadata| metadata.file_type().is_file()) {
        return Err("source is missing or is not a file".into());
    }
    if std::fs::symlink_metadata(&target).is_ok() {
        return Err("output already exists; refusing to overwrite".into());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !io::stderr().is_terminal() {
        return Err(
            "secure interactive terminal required: piped/redirected input and passphrase flags or environment variables are refused"
                .into(),
        );
    }
    let limits = max_limits(max_mib).map_err(|error| error.to_string())?;
    let service = BackupService::new(limits);
    let passphrase = password_prompt("Backup passphrase (hidden): ").map_err(str::to_string)?;
    if creating_backup {
        let confirmation =
            password_prompt("Confirm passphrase (hidden): ").map_err(str::to_string)?;
        if passphrase.as_bytes() != confirmation.as_bytes() {
            return Err("passphrases do not match; nothing was written".into());
        }
    }
    if creating_backup {
        service
            .backup_database(&source, &target, passphrase.as_bytes())
            .map_err(|error| error.to_string())?;
        println!("Encrypted SQLite backup saved to {}", target.display());
    } else {
        service
            .restore_database(&source, &target, passphrase.as_bytes())
            .map_err(|error| error.to_string())?;
        println!("Verified SQLite database restored to {}", target.display());
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("Error: {message}");
            eprintln!("Run constellation-backup-cli --help for instructions.");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_password_flags_and_unexpected_arguments() {
        let args = ["backup", "a.sqlite3", "new.cbackup", "--password", "secret"];
        assert!(parse(args.map(str::to_string)).is_err());
        let args = ["restore", "a.cbackup", "new.sqlite3", "--max-mib", "65"];
        assert!(parse(args.map(str::to_string)).is_err());
        assert!(parse(["backup", "src", "-password"].map(str::to_string)).is_err());
    }

    #[test]
    fn parses_both_commands_and_enforces_size_limits() {
        let args = ["backup", "src", "dst", "--max-mib", "64"];
        assert!(matches!(
            parse(args.map(str::to_string)),
            Ok(Command::Backup { max_mib: 64, .. })
        ));
        let args = ["restore", "src", "dst"];
        assert!(matches!(
            parse(args.map(str::to_string)),
            Ok(Command::Restore { max_mib: 16, .. })
        ));
        assert_eq!(parse(["--help".to_string()]), Ok(Command::Help));
    }
}
