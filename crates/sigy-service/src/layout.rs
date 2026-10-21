//! Default library, state, and cache locations.
//!
//! Windows and macOS keep `~/.sigy`. Linux uses the XDG base directories for a
//! new install. An existing `~/.sigy/library` stays in use when the XDG library
//! is absent. Relative XDG values are ignored.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::{Error, Result};

#[derive(Clone, Debug, Default)]
pub struct OwnedEnv {
    pub data_home: Option<PathBuf>,
    pub state_home: Option<PathBuf>,
    pub cache_home: Option<PathBuf>,
    pub runtime_dir: Option<PathBuf>,
}

#[derive(Debug)]
pub struct Input<'a> {
    pub home: &'a Path,
    pub data_home: Option<&'a Path>,
    pub state_home: Option<&'a Path>,
    pub cache_home: Option<&'a Path>,
    pub legacy_library: bool,
    pub xdg_library: bool,
    pub linux: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Places {
    pub library: PathBuf,
    pub state_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub legacy: bool,
}

/// The absolute user home.
///
/// # Errors
/// Returns an error when that home is missing, relative, empty, or has no parent.
pub fn home_dir() -> std::result::Result<PathBuf, &'static str> {
    #[cfg(windows)]
    let value = env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let value = env::var_os("HOME");
    value
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.parent().is_some())
        .ok_or("a nonempty absolute user home directory is required; supply --data-dir PATH explicitly")
}

#[must_use]
pub fn owned_env() -> OwnedEnv {
    OwnedEnv {
        data_home: env_path("XDG_DATA_HOME"),
        state_home: env_path("XDG_STATE_HOME"),
        cache_home: env_path("XDG_CACHE_HOME"),
        runtime_dir: env_path("XDG_RUNTIME_DIR"),
    }
}

#[must_use]
pub fn places(input: &Input<'_>) -> Places {
    let legacy_root = input.home.join(".sigy");
    if !input.linux || (input.legacy_library && !input.xdg_library) {
        return Places {
            library: legacy_root.join("library"),
            state_dir: legacy_root.clone(),
            cache_dir: legacy_root,
            legacy: input.linux && input.legacy_library && !input.xdg_library,
        };
    }
    Places {
        library: xdg_dir(input.home, input.data_home, ".local/share").join("library"),
        state_dir: xdg_dir(input.home, input.state_home, ".local/state"),
        cache_dir: xdg_dir(input.home, input.cache_home, ".cache"),
        legacy: false,
    }
}

#[must_use]
pub fn process_places(home: &Path) -> Places {
    let env = owned_env();
    let (legacy_library, xdg_library) = probe(home, &env);
    places(&Input {
        home,
        data_home: env.data_home.as_deref(),
        state_home: env.state_home.as_deref(),
        cache_home: env.cache_home.as_deref(),
        legacy_library,
        xdg_library,
        linux: cfg!(target_os = "linux"),
    })
}

#[must_use]
pub fn state_file(home: &Path, places: &Places, name: &str) -> PathBuf {
    let legacy = home.join(".sigy").join(name);
    if places.legacy {
        return legacy;
    }
    let chosen = places.state_dir.join(name);
    if legacy.is_file() && !chosen.exists() {
        legacy
    } else {
        chosen
    }
}

#[must_use]
pub fn source_checkout(home: &Path, places: &Places) -> PathBuf {
    let legacy = home.join(".sigy").join("src");
    if places.legacy {
        return legacy;
    }
    let chosen = places.cache_dir.join("src");
    if legacy.is_dir() && !chosen.exists() {
        legacy
    } else {
        chosen
    }
}

/// The socket directory for one library.
///
/// An absolute runtime directory is used only for the default Linux library.
/// The caller checks that the runtime directory exists. A relative value stays
/// beside the library, as does every explicit library.
#[must_use]
pub fn socket_parent(
    library: &Path,
    default_library: &Path,
    runtime: Option<&Path>,
    linux: bool,
) -> PathBuf {
    let runtime = runtime.filter(|path| path.is_absolute());
    if linux
        && same_directory(library, default_library)
        && let Some(runtime) = runtime
    {
        return runtime.join("sigy");
    }
    library.to_path_buf()
}

#[must_use]
pub fn is_legacy_default(directory: &Path, places: &Places) -> bool {
    places.legacy && same_directory(directory, &places.library)
}

/// Keeps a runtime directory only when it already exists.
#[must_use]
pub fn existing_runtime(runtime: Option<&Path>) -> Option<&Path> {
    runtime.filter(|path| path.is_absolute() && path.is_dir())
}

#[must_use]
pub fn same_directory(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[must_use]
pub fn socket_parent_for(library: &Path) -> PathBuf {
    let Ok(home) = home_dir() else {
        return library.to_path_buf();
    };
    let env = owned_env();
    let places = process_places(&home);
    socket_parent(
        library,
        &places.library,
        existing_runtime(env.runtime_dir.as_deref()),
        cfg!(target_os = "linux"),
    )
}

#[cfg(unix)]
pub(crate) fn create_socket_directory(path: &Path) -> Result<()> {
    create_private(path)
}

/// Move a legacy Linux library, its update files, and its source checkout.
///
/// # Errors
/// Refuses another operating system, a missing legacy library, an existing XDG
/// library, an existing state or cache file, a running service, or a
/// cross-volume rename. A failed later step restores any files already moved.
pub fn relocate_legacy_library() -> Result<PathBuf> {
    if !cfg!(target_os = "linux") {
        return Err(Error::InvalidInput(
            "XDG library locations are used on Linux",
        ));
    }
    let home = home_dir().map_err(|_| Error::InvalidInput("the home directory is not set"))?;
    relocate_in(&home, &owned_env(), true)
}

/// Move one home's legacy library using the supplied environment.
///
/// # Errors
/// Returns an error for a non-Linux layout, a missing or busy legacy library,
/// an existing XDG library or state file, or a failed rename.
pub fn relocate_in(home: &Path, env: &OwnedEnv, linux: bool) -> Result<PathBuf> {
    if !linux {
        return Err(Error::InvalidInput(
            "XDG library locations are used on Linux",
        ));
    }
    let (legacy_library, xdg_library) = probe(home, env);
    if !legacy_library {
        return Err(Error::InvalidInput("there is no legacy library to move"));
    }
    if xdg_library {
        return Err(Error::InvalidInput("the XDG library already exists"));
    }
    let current = places(&input(home, env, true, false));
    let fresh = places(&input(home, env, false, false));
    match crate::library::Library::open(&current.library, false) {
        Ok(library) => drop(library),
        Err(Error::LibraryBusy) => return Err(Error::LibraryBusy),
        Err(Error::InvalidInput("library has not been initialized")) => {}
        Err(error) => return Err(error),
    }
    let mut undo = Vec::new();
    let moved = (|| -> Result<()> {
        for name in ["update-status.json", "installed-commit"] {
            if let Some(moved) =
                move_if_present(&home.join(".sigy").join(name), &fresh.state_dir.join(name))?
            {
                undo.push(moved);
            }
        }
        if let Some(moved) = move_if_present(
            &home.join(".sigy").join("src"),
            &fresh.cache_dir.join("src"),
        )? {
            undo.push(moved);
        }
        rename_library(&current.library, &fresh.library)
    })();
    if let Err(error) = moved {
        for moved in undo.into_iter().rev() {
            let _ = fs::rename(&moved.to, &moved.from);
        }
        return Err(error);
    }
    Ok(fresh.library)
}

fn input<'a>(
    home: &'a Path,
    env: &'a OwnedEnv,
    legacy_library: bool,
    xdg_library: bool,
) -> Input<'a> {
    Input {
        home,
        data_home: env.data_home.as_deref(),
        state_home: env.state_home.as_deref(),
        cache_home: env.cache_home.as_deref(),
        legacy_library,
        xdg_library,
        linux: true,
    }
}

fn probe(home: &Path, env: &OwnedEnv) -> (bool, bool) {
    let legacy = home.join(".sigy").join("library");
    let fresh = places(&input(home, env, false, false)).library;
    (is_real_dir(&legacy), is_real_dir(&fresh))
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn xdg_dir(home: &Path, override_path: Option<&Path>, fallback: &str) -> PathBuf {
    absolute(override_path).map_or_else(
        || home.join(fallback).join("sigy"),
        |path| path.join("sigy"),
    )
}

fn absolute(path: Option<&Path>) -> Option<&Path> {
    path.filter(|path| path.is_absolute())
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

struct Moved {
    from: PathBuf,
    to: PathBuf,
}

fn move_if_present(from: &Path, to: &Path) -> Result<Option<Moved>> {
    if fs::symlink_metadata(from).is_err() {
        return Ok(None);
    }
    if fs::symlink_metadata(to).is_ok() {
        return Err(Error::InvalidInput(
            "an XDG state or cache file already exists",
        ));
    }
    if let Some(parent) = to.parent() {
        create_private(parent)?;
    }
    fs::rename(from, to)?;
    Ok(Some(Moved {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
    }))
}

fn rename_library(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        create_private(parent)?;
    }
    fs::rename(from, to).map_err(|error| {
        if error.raw_os_error() == Some(18) {
            Error::InvalidInput("the library and the XDG directory are on different volumes")
        } else {
            Error::Io(error)
        }
    })
}

fn create_private(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/home/ada")
    }

    /// A directory the host treats as absolute. Unix-looking paths are not
    /// absolute on Windows, so the Linux rules are tested with a real prefix.
    fn absolute_dir(path: &str) -> PathBuf {
        let trimmed = path.trim_start_matches('/');
        #[cfg(windows)]
        {
            PathBuf::from(format!(r"C:\sigy-xdg\{}", trimmed.replace('/', "\\")))
        }
        #[cfg(not(windows))]
        {
            PathBuf::from(format!("/{trimmed}"))
        }
    }

    #[test]
    fn windows_and_macos_keep_the_home_library() {
        let home = home();
        let places = places(&Input {
            home: &home,
            data_home: Some(Path::new("/opt/share")),
            state_home: Some(Path::new("/opt/state")),
            cache_home: Some(Path::new("/opt/cache")),
            legacy_library: true,
            xdg_library: false,
            linux: false,
        });
        assert_eq!(places.library, home.join(".sigy/library"));
        assert!(!places.legacy);
        assert_eq!(places.state_dir, home.join(".sigy"));
    }

    #[test]
    fn a_new_linux_library_uses_xdg_and_ignores_relative_values() {
        let home = home();
        let cache = absolute_dir("/var/cache");
        let places = places(&Input {
            home: &home,
            data_home: Some(Path::new("relative")),
            state_home: None,
            cache_home: Some(&cache),
            legacy_library: false,
            xdg_library: false,
            linux: true,
        });
        assert_eq!(places.library, home.join(".local/share/sigy/library"));
        assert_eq!(places.state_dir, home.join(".local/state/sigy"));
        assert_eq!(places.cache_dir, cache.join("sigy"));
        assert!(!places.legacy);
    }

    #[test]
    fn an_existing_linux_library_stays_until_an_xdg_library_exists() {
        let home = home();
        let share = absolute_dir("/opt/share");
        let legacy = places(&Input {
            home: &home,
            data_home: Some(&share),
            state_home: None,
            cache_home: None,
            legacy_library: true,
            xdg_library: false,
            linux: true,
        });
        assert!(legacy.legacy);
        assert!(is_legacy_default(&legacy.library, &legacy));
        assert!(!is_legacy_default(&PathBuf::from("/tmp/other"), &legacy));
        assert_eq!(legacy.library, home.join(".sigy/library"));
        let both = places(&Input {
            home: &home,
            data_home: Some(&share),
            state_home: None,
            cache_home: None,
            legacy_library: true,
            xdg_library: true,
            linux: true,
        });
        assert!(!both.legacy);
        assert!(!is_legacy_default(&both.library, &both));
        assert_eq!(both.library, share.join("sigy").join("library"));
    }

    #[test]
    fn the_runtime_socket_is_per_default_library_and_absolute() {
        let library = PathBuf::from("/home/ada/.local/share/sigy/library");
        let other = PathBuf::from("/tmp/other");
        let runtime = absolute_dir("/run/user/1000");
        assert_eq!(
            socket_parent(&library, &library, Some(&runtime), true),
            runtime.join("sigy")
        );
        assert_eq!(socket_parent(&other, &library, Some(&runtime), true), other);
        assert_eq!(
            socket_parent(&library, &library, Some(Path::new("relative")), true),
            library
        );
        assert_eq!(
            socket_parent(&library, &library, Some(&runtime), false),
            library
        );
    }

    #[test]
    fn a_missing_runtime_directory_is_not_a_socket_parent() -> Result<()> {
        let root = tempfile::tempdir()?;
        let missing = root.path().join("missing");
        let present = root.path().join("run");
        fs::create_dir(&present)?;
        assert_eq!(existing_runtime(Some(&missing)), None);
        assert_eq!(existing_runtime(Some(Path::new("relative"))), None);
        assert_eq!(existing_runtime(Some(&present)), Some(present.as_path()));
        Ok(())
    }

    #[test]
    fn relocate_moves_the_library_state_and_checkout_together() -> Result<()> {
        let root = tempfile::tempdir()?;
        let home = root.path().join("home");
        let legacy = home.join(".sigy/library");
        fs::create_dir_all(&legacy)?;
        fs::write(legacy.join("note.txt"), b"kept")?;
        fs::write(home.join(".sigy/update-status.json"), b"{}")?;
        fs::create_dir_all(home.join(".sigy/src"))?;
        fs::write(home.join(".sigy/src/input.txt"), b"source")?;
        let destination = relocate_in(&home, &OwnedEnv::default(), true)?;
        assert_eq!(destination, home.join(".local/share/sigy/library"));
        assert_eq!(fs::read(destination.join("note.txt"))?, b"kept");
        assert!(!legacy.exists());
        assert_eq!(
            fs::read(home.join(".local/state/sigy/update-status.json"))?,
            b"{}"
        );
        assert_eq!(fs::read(home.join(".cache/sigy/src/input.txt"))?, b"source");
        assert!(relocate_in(&home, &OwnedEnv::default(), true).is_err());
        Ok(())
    }

    #[test]
    fn relocate_restores_a_moved_receipt_when_a_later_file_exists() -> Result<()> {
        let root = tempfile::tempdir()?;
        let home = root.path().join("home");
        let legacy = home.join(".sigy/library");
        fs::create_dir_all(&legacy)?;
        fs::write(home.join(".sigy/update-status.json"), b"{}")?;
        fs::write(home.join(".sigy/installed-commit"), b"old\n")?;
        let state = home.join(".local/state/sigy");
        fs::create_dir_all(&state)?;
        fs::write(state.join("installed-commit"), b"new\n")?;
        match relocate_in(&home, &OwnedEnv::default(), true) {
            Err(crate::Error::InvalidInput(message)) => {
                assert_eq!(message, "an XDG state or cache file already exists");
            }
            Ok(path) => panic!("move should refuse an existing state file, got {path:?}"),
            Err(error) => panic!("unexpected move error: {error}"),
        }
        assert!(legacy.exists());
        assert_eq!(fs::read(home.join(".sigy/update-status.json"))?, b"{}");
        assert!(!state.join("update-status.json").exists());
        assert_eq!(fs::read(home.join(".sigy/installed-commit"))?, b"old\n");
        Ok(())
    }

    #[test]
    fn an_existing_state_file_remains_readable_before_the_xdg_file_exists() -> Result<()> {
        let root = tempfile::tempdir()?;
        let home = root.path().join("home");
        fs::create_dir_all(home.join(".sigy"))?;
        fs::write(home.join(".sigy/installed-commit"), b"abc\n")?;
        let places = places(&Input {
            home: &home,
            data_home: None,
            state_home: None,
            cache_home: None,
            legacy_library: false,
            xdg_library: false,
            linux: true,
        });
        assert_eq!(
            state_file(&home, &places, "installed-commit"),
            home.join(".sigy/installed-commit")
        );
        Ok(())
    }
}
