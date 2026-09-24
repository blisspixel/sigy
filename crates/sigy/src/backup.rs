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
    if cli.json {
        serde_json::to_writer(&mut stdout, manifest)?;
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
