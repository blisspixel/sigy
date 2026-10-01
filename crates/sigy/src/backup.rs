//! `library backup`, `library verify-backup` and `library restore`.

use std::io::{self, Write};

use sigy_service::{
    Error,
    backup::{self, BackupManifest},
    library::Library,
};

use crate::{Cli, LibraryCommand, library_dir};

/// Runs the backup commands locally. Returns `None` for other library commands.
pub fn execute(
    cli: &Cli,
    command: &LibraryCommand,
) -> Option<Result<(), Box<dyn std::error::Error>>> {
    let result = match command {
        LibraryCommand::Backup { destination } => library_dir(cli).and_then(|directory| {
            let library = Library::open(directory, false).map_err(|error| match error {
                Error::LibraryBusy => {
                    "the service holds this library; stop it with service stop, then back up".into()
                }
                other => Box::<dyn std::error::Error>::from(other),
            })?;
            let manifest = backup::backup(&library, destination)?;
            report(cli, "Backup written", destination, &manifest)
        }),
        LibraryCommand::VerifyBackup { backup: source } => backup::verify(source)
            .map_err(Into::into)
            .and_then(|manifest| report(cli, "Backup verified", source, &manifest)),
        LibraryCommand::Restore {
            backup: source,
            into,
        } => backup::restore(source, into)
            .map_err(Into::into)
            .and_then(|manifest| report(cli, "Library restored", into, &manifest)),
        LibraryCommand::Init | LibraryCommand::Status => return None,
    };
    Some(result)
}

fn report(
    cli: &Cli,
    action: &str,
    path: &std::path::Path,
    manifest: &BackupManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = io::stdout().lock();
    render_report(&mut stdout, cli.json, action, path, manifest)
}

fn render_report(
    stdout: &mut impl Write,
    json: bool,
    action: &str,
    path: &std::path::Path,
    manifest: &BackupManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    if json {
        serde_json::to_writer(&mut *stdout, manifest)?;
        writeln!(stdout)?;
        return Ok(());
    }
    writeln!(
        stdout,
        "{action}: {}",
        crate::explorer::text::sanitize(&path.display().to_string(), 1024)
    )?;
    writeln!(
        stdout,
        "Schema v{} | catalog {} bytes, sha256 {} | {} media objects, {} bytes.",
        manifest.schema_version,
        manifest.catalog.bytes,
        manifest.catalog.sha256,
        manifest.media.len(),
        manifest.media_bytes
    )?;
    writeln!(
        stdout,
        "Every file matches its SHA-256. Hashes detect corruption; they are not signatures."
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::path::Path;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn run(words: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
        let cli = Cli::try_parse_from(words)?;
        let crate::Command::Library { command } = &cli.command else {
            return Err("not a library command".into());
        };
        execute(&cli, command).ok_or("not handled by backup")?
    }

    #[test]
    fn cli_backup_roundtrip_preserves_library_and_refuses_corruption() -> TestResult {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("source");
        let destination = temp.path().join("backup");
        let restored = temp.path().join("restored");
        let source_text = source.to_str().ok_or("source path")?;
        let backup_text = destination.to_str().ok_or("backup path")?;
        let restored_text = restored.to_str().ok_or("restore path")?;
        let owner = Library::open(&source, true)?;
        let blocked = run(&[
            "sigy",
            "--data-dir",
            source_text,
            "library",
            "backup",
            backup_text,
        ]);
        assert!(
            blocked
                .err()
                .ok_or("backup should refuse a busy library")?
                .to_string()
                .contains("stop it with service stop")
        );
        assert!(!destination.exists());
        drop(owner);
        run(&[
            "sigy",
            "--data-dir",
            source_text,
            "library",
            "backup",
            backup_text,
        ])?;
        run(&["sigy", "--json", "library", "verify-backup", backup_text])?;
        run(&[
            "sigy",
            "library",
            "restore",
            backup_text,
            "--into",
            restored_text,
        ])?;
        drop(Library::open(&restored, false)?);
        assert!(
            run(&[
                "sigy",
                "library",
                "restore",
                backup_text,
                "--into",
                source_text
            ])
            .is_err()
        );
        std::fs::write(destination.join("catalog.sqlite3"), b"damaged catalog")?;
        assert!(run(&["sigy", "library", "verify-backup", backup_text]).is_err());
        let refused = temp.path().join("refused");
        assert!(
            run(&[
                "sigy",
                "library",
                "restore",
                backup_text,
                "--into",
                refused.to_str().ok_or("path")?
            ])
            .is_err()
        );
        assert!(!refused.exists());
        Ok(())
    }

    #[test]
    fn backup_requires_existing_library_and_leaves_other_library_commands_alone() -> TestResult {
        let cli = Cli::try_parse_from(["sigy", "library", "status"])?;
        assert!(execute(&cli, &LibraryCommand::Status).is_none());
        assert!(execute(&cli, &LibraryCommand::Init).is_none());
        let temp = tempfile::tempdir()?;
        let missing = temp.path().join("absent");
        let destination = temp.path().join("destination");
        let destination_text = destination.to_str().ok_or("path")?;
        assert!(run(&["sigy", "library", "backup", destination_text]).is_err());
        assert!(
            run(&[
                "sigy",
                "--data-dir",
                missing.to_str().ok_or("path")?,
                "library",
                "backup",
                destination_text
            ])
            .is_err()
        );
        assert!(!destination.exists());
        assert!(run(&["sigy", "library", "verify-backup", destination_text]).is_err());
        Ok(())
    }

    #[test]
    fn report_retains_exact_manifest_and_sanitizes_terminal_path() -> TestResult {
        let manifest = BackupManifest {
            format: backup::BACKUP_FORMAT.into(),
            schema_version: 39,
            created_ms: 123,
            catalog: backup::BackupFile {
                bytes: 9_007_199_254_740_993,
                sha256: "a".repeat(64),
            },
            media: Vec::new(),
            media_bytes: 0,
        };
        let hostile_path = Path::new("أرشيف\u{1b}]52;c;payload\u{7}\nforged");
        let mut bytes = Vec::new();
        render_report(
            &mut bytes,
            false,
            "Backup verified",
            hostile_path,
            &manifest,
        )?;
        let text = String::from_utf8(bytes)?;
        assert!(text.contains("أرشيف"));
        assert!(text.contains("catalog 9007199254740993 bytes"));
        assert!(text.contains("Hashes detect corruption; they are not signatures."));
        assert!(!text.chars().any(|ch| ch.is_control() && ch != '\n'));
        assert_eq!(text.lines().count(), 3);
        bytes = Vec::new();
        render_report(&mut bytes, true, "Backup verified", hostile_path, &manifest)?;
        let read: BackupManifest = serde_json::from_slice(&bytes)?;
        assert_eq!(read, manifest);
        assert!(!String::from_utf8(bytes)?.contains("payload"));
        Ok(())
    }
}
