//! Verifies scripts in the repository have valid syntax.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) fn check_scripts(root: &Path) -> Result<(), String> {
    let install_sh = root.join("scripts").join("install.sh");
    if !install_sh.is_file() {
        return Err("scripts/install.sh is missing".into());
    }

    if let Some(sh) = find_sh() {
        let clean_path = clean_path_for_sh(&install_sh);
        let output = Command::new(&sh)
            .arg("-n")
            .arg(&clean_path)
            .output()
            .map_err(|e| format!("failed to execute {}: {e}", sh.display()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "scripts/install.sh syntax error via {}: {stderr}",
                sh.display()
            ));
        }
    } else if cfg!(unix) {
        return Err("sh was not found to verify scripts/install.sh".into());
    }

    Ok(())
}

fn clean_path_for_sh(path: &Path) -> String {
    let s = path.to_string_lossy();
    let stripped = s.strip_prefix(r"\\?\").unwrap_or(&s);
    stripped.replace('\\', "/")
}

fn find_sh() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("SIGY_TEST_SH") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }

    if let Some(path) = find_on_path("sh") {
        return Some(path);
    }

    #[cfg(windows)]
    {
        if let Some(git_path) = find_on_path("git")
            && let Some(parent) = git_path.parent().and_then(Path::parent)
        {
            let bin_sh = parent.join("bin").join("sh.exe");
            if bin_sh.is_file() {
                return Some(bin_sh);
            }
            let usr_bin_sh = parent.join("usr").join("bin").join("sh.exe");
            if usr_bin_sh.is_file() {
                return Some(usr_bin_sh);
            }
        }
        let fallback = PathBuf::from(r"C:\Program Files\Git\bin\sh.exe");
        if fallback.is_file() {
            return Some(fallback);
        }
    }

    None
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let extensions = if cfg!(windows) {
        std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
    } else {
        String::new()
    };
    let mut suffixes = vec![String::new()];
    suffixes.extend(extensions.split(';').map(str::to_owned));
    for directory in std::env::split_paths(&path) {
        for suffix in &suffixes {
            let candidate = directory.join(format!("{name}{suffix}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_sh_syntax_check_passes_on_workspace() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest
            .parent()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."));
        assert!(check_scripts(root).is_ok());
    }

    #[test]
    fn install_sh_syntax_check_detects_syntax_errors() {
        let Some(sh) = find_sh() else { return };
        let temp_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let scripts_dir = temp_dir.path().join("scripts");
        std::fs::create_dir_all(&scripts_dir).unwrap_or_else(|e| panic!("create_dir: {e}"));
        let bad_script = scripts_dir.join("install.sh");
        std::fs::write(&bad_script, b"if [ -f foo ]; then echo hi }\n")
            .unwrap_or_else(|e| panic!("write: {e}"));

        let clean_path = clean_path_for_sh(&bad_script);
        let output = Command::new(&sh).arg("-n").arg(&clean_path).output();
        if let Ok(output) = output {
            assert!(!output.status.success());
        }
    }
}
