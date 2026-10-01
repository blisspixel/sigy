use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use scorer_probe::{judge_records::CalibrationSet, selection::sha256};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{RUBRIC, Result, SCHEMA, TEMPLATE};

const MODEL_SHA: &str = "8e30dff3ac4c8434c49a7036fa15564bdbb6044e42bf04550bf1a096ad7e6a52";
const EXE_SHA: &str = "3427f711f8d20ddb4f141cd89f3ef0c4351e389fa5d54af1d503ff778560518a";
const ZIP_SHA: &str = "14cf1303ca9ac3abd94816850532f9f9a69ac66fbaca3776fc6f9061c2fac1d1";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema_version: u32,
    pub frozen_unix_seconds: u64,
    pub runner_source_sha256: String,
    pub runner_binary: Asset,
    pub declared_build_mode: String,
    pub scorer_source_sha256: String,
    pub controls_sha256: String,
    pub blinded_inputs_sha256: String,
    pub rubric_sha256: String,
    pub output_schema_sha256: String,
    pub template_sha256: String,
    pub order_sha256: String,
    pub runtime: PathBuf,
    pub runtime_files: Vec<Asset>,
    pub runtime_archive: Asset,
    pub model: Asset,
    pub arguments: Vec<String>,
    pub limits: Limits,
    pub limitations: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub processes: u32,
    pub threads: u32,
    pub memory_bytes: u64,
    pub cpu_processors: u32,
    pub call_seconds: u64,
    pub batch_seconds: u64,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            processes: 1,
            threads: 2,
            memory_bytes: 8 * 1024 * 1024 * 1024,
            cpu_processors: 2,
            call_seconds: 90,
            batch_seconds: 45 * 60,
            stdout_bytes: 64 * 1024,
            stderr_bytes: 256 * 1024,
        }
    }
}

pub fn arguments() -> Vec<String> {
    [
        "--offline",
        "-no-cnv",
        "--no-display-prompt",
        "--no-warmup",
        "-n",
        "256",
        "-c",
        "4096",
        "--temp",
        "0",
        "-s",
        "0",
        "-t",
        "2",
        "-dev",
        "none",
        "-ngl",
        "0",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

pub fn plain(path: &Path, directory: bool) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err("asset must be a plain file or directory".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("reparse point refused".into());
        }
    }
    Ok(())
}

pub fn read(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    plain(path, false)?;
    let mut bytes = Vec::new();
    File::open(path)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err("artifact exceeds byte ceiling".into());
    }
    Ok(bytes)
}

pub fn pinned(path: &Path, expected: &str, maximum: u64) -> Result<Vec<u8>> {
    let bytes = read(path, maximum)?;
    if !scorer_probe::selection::is_sha256(expected) || sha256(&bytes) != expected {
        return Err("artifact digest mismatch".into());
    }
    Ok(bytes)
}

pub fn hash(path: &Path, maximum: u64) -> Result<Asset> {
    plain(path, false)?;
    let mut file = File::open(path)?;
    let declared = file.metadata()?.len();
    if declared == 0 || declared > maximum {
        return Err("asset byte ceiling exceeded or empty".into());
    }
    let mut bytes = 0_u64;
    let mut buffer = vec![0; 256 * 1024];
    let mut hasher = Sha256::new();
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > maximum {
            return Err("asset grew past byte ceiling".into());
        }
        hasher.update(&buffer[..count]);
    }
    if bytes != declared {
        return Err("asset changed during hashing".into());
    }
    Ok(Asset {
        path: path.to_owned(),
        bytes,
        sha256: scorer_probe::hex(&hasher.finalize()),
    })
}

fn runtime_files(runtime: &Path) -> Result<Vec<Asset>> {
    plain(runtime, true)?;
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(runtime)? {
        let entry = entry?;
        plain(&entry.path(), false)?;
        paths.push(entry.path());
        if paths.len() > 256 {
            return Err("runtime file ceiling exceeded".into());
        }
    }
    paths.sort();
    let mut assets = Vec::new();
    let mut total = 0;
    for path in paths {
        let item = hash(&path, 1024 * 1024 * 1024 - total)?;
        total += item.bytes;
        assets.push(item);
    }
    let executable = runtime.join("llama-completion.exe");
    if !assets
        .iter()
        .any(|asset| asset.path == executable && asset.sha256 == EXE_SHA)
    {
        return Err("pinned completion executable missing".into());
    }
    Ok(assets)
}

pub fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub fn json_new(path: &Path, value: &impl Serialize) -> Result<()> {
    write_new(path, &serde_json::to_vec_pretty(value)?)
}

fn verify_archive(archive: &Asset, files: &[Asset]) -> Result<()> {
    let mut zip = zip::ZipArchive::new(File::open(&archive.path)?)?;
    if zip.len() != files.len() || zip.len() > 256 {
        return Err("archive/runtime file count mismatch".into());
    }
    let mut names = std::collections::BTreeSet::new();
    let mut total = 0_u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let name = entry.name().to_owned();
        if entry.is_dir()
            || Path::new(&name).components().count() != 1
            || name.contains(['\\', ':'])
            || !names.insert(name.clone())
            || entry.size() > 64 * 1024 * 1024
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170_000 == 0o120_000)
        {
            return Err("unsafe or oversized archive entry".into());
        }
        let expected = files
            .iter()
            .find(|asset| {
                asset
                    .path
                    .file_name()
                    .is_some_and(|file| file == name.as_str())
            })
            .ok_or("archive entry absent from runtime")?;
        let mut bytes = Vec::new();
        (&mut entry)
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        total += bytes.len() as u64;
        if total > 1024 * 1024 * 1024
            || bytes.len() as u64 != entry.size()
            || bytes.len() as u64 != expected.bytes
            || sha256(&bytes) != expected.sha256
        {
            return Err("archive/runtime member digest mismatch".into());
        }
    }
    Ok(())
}

pub fn freeze(
    controls: &CalibrationSet,
    runtime: &Path,
    model: &Path,
    directory: &Path,
) -> Result<()> {
    let runtime = std::fs::canonicalize(runtime)?;
    let model = std::fs::canonicalize(model)?;
    let files = runtime_files(&runtime)?;
    let model = hash(&model, 4 * 1024 * 1024 * 1024)?;
    if model.sha256 != MODEL_SHA {
        return Err("only the existing pinned Gemma model is allowed".into());
    }
    let zip = runtime
        .parent()
        .ok_or("runtime parent missing")?
        .join("downloads/llama-b11146-bin-win-cpu-x64.zip");
    let runtime_archive = hash(&zip, 32 * 1024 * 1024)?;
    if runtime_archive.sha256 != ZIP_SHA {
        return Err("pinned runtime archive mismatch".into());
    }
    verify_archive(&runtime_archive, &files)?;
    let bytes = serde_json::to_vec_pretty(controls)?;
    let control_digest = sha256(&bytes);
    let input = scorer_probe::judge_scoring::BlindedInputs {
        schema_version: 1,
        control_artifact_sha256: &control_digest,
        rubric_sha256: &controls.rubric_sha256,
        records: controls
            .controls
            .iter()
            .map(|item| scorer_probe::judge_scoring::BlindedInput {
                control_id: &item.control_id,
                source_text: &item.source_text,
                english_reference: &item.english_reference,
                output_text: &item.output_text,
                untrusted_context: &item.untrusted_context,
            })
            .collect(),
    };
    let inputs = serde_json::to_vec_pretty(&input)?;
    let order: Vec<_> = controls
        .controls
        .iter()
        .map(|control| &control.control_id)
        .collect();
    let profile = Profile {
        schema_version: 1,
        frozen_unix_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        runner_source_sha256: crate::source_digest(),
        runner_binary: hash(&std::env::current_exe()?, 64 * 1024 * 1024)?,
        declared_build_mode: std::env::var("SIGY_JUDGE_BUILD_MODE").unwrap_or_else(|_| "unclassified local build".into()),
        scorer_source_sha256: scorer_probe::scoring::source_bundle_digest(),
        controls_sha256: control_digest,
        blinded_inputs_sha256: sha256(&inputs),
        rubric_sha256: sha256(RUBRIC.as_bytes()),
        output_schema_sha256: sha256(SCHEMA.as_bytes()),
        template_sha256: sha256(TEMPLATE.as_bytes()),
        order_sha256: sha256(&serde_json::to_vec(&order)?),
        runtime,
        runtime_files: files,
        runtime_archive,
        model,
        arguments: arguments(),
        limits: Limits::default(),
        limitations: "Reference-assisted declared controls, not independent semantic gold. CPU only. OS network isolation is unproven; --offline is an application option. Existing benchmark exposure unknown. Every local runtime file matches its named member in the pinned upstream archive; OS/driver/system DLL closure is not included. Build mode is a declaration; the exact runner binary is hashed.".into(),
    };
    if let Some(parent) = directory.parent() {
        std::fs::create_dir_all(parent)?;
        plain(parent, true)?;
    }
    std::fs::create_dir(directory)?;
    plain(directory, true)?;
    write_new(&directory.join("controls.json"), &bytes)?;
    write_new(&directory.join("blinded-inputs.json"), &inputs)?;
    write_new(&directory.join("rubric.txt"), RUBRIC.as_bytes())?;
    write_new(&directory.join("output-schema.json"), SCHEMA.as_bytes())?;
    json_new(&directory.join("profile.json"), &profile)?;
    println!(
        "frozen_controls={} profile_sha256={}",
        controls.controls.len(),
        sha256(&serde_json::to_vec_pretty(&profile)?)
    );
    Ok(())
}

pub fn validate_profile(profile: &Profile, directory: &Path) -> Result<()> {
    plain(directory, true)?;
    if hash(&std::env::current_exe()?, 64 * 1024 * 1024)? != profile.runner_binary {
        return Err("frozen runner binary changed".into());
    }
    if profile.schema_version != 1
        || profile.runner_source_sha256 != crate::source_digest()
        || profile.scorer_source_sha256 != scorer_probe::scoring::source_bundle_digest()
        || profile.rubric_sha256 != sha256(RUBRIC.as_bytes())
        || profile.output_schema_sha256 != sha256(SCHEMA.as_bytes())
        || profile.template_sha256 != sha256(TEMPLATE.as_bytes())
        || profile.limits != Limits::default()
        || profile.arguments != arguments()
    {
        return Err("frozen profile differs from this fixed runner".into());
    }
    pinned(&directory.join("rubric.txt"), &profile.rubric_sha256, 8192)?;
    pinned(
        &directory.join("output-schema.json"),
        &profile.output_schema_sha256,
        8192,
    )?;
    pinned(
        &directory.join("blinded-inputs.json"),
        &profile.blinded_inputs_sha256,
        2 * 1024 * 1024,
    )?;
    if runtime_files(&profile.runtime)? != profile.runtime_files
        || hash(&profile.model.path, 4 * 1024 * 1024 * 1024)? != profile.model
        || profile.model.sha256 != MODEL_SHA
        || hash(&profile.runtime_archive.path, 32 * 1024 * 1024)? != profile.runtime_archive
        || profile.runtime_archive.sha256 != ZIP_SHA
    {
        return Err("frozen native closure changed".into());
    }
    verify_archive(&profile.runtime_archive, &profile.runtime_files)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch {
        root: PathBuf,
        files: Vec<PathBuf>,
    }

    impl Scratch {
        fn new() -> Result<Self> {
            let root = std::env::temp_dir().join(format!(
                "sigy-judge-contract-{}-{}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
            ));
            std::fs::create_dir(&root)?;
            Ok(Self {
                root,
                files: Vec::new(),
            })
        }
        fn file(&mut self, name: &str, bytes: &[u8]) -> Result<PathBuf> {
            let path = self.root.join(name);
            write_new(&path, bytes)?;
            self.files.push(path.clone());
            Ok(path)
        }
        fn archive(&mut self, name: &str, entry: &str, bytes: &[u8]) -> Result<Asset> {
            let path = self.root.join(name);
            let mut writer = zip::ZipWriter::new(
                OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&path)?,
            );
            self.files.push(path.clone());
            writer.start_file(
                entry,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )?;
            writer.write_all(bytes)?;
            writer.finish()?.sync_all()?;
            hash(&path, 1024 * 1024)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            for path in &self.files {
                let _ = std::fs::remove_file(path);
            }
            let _ = std::fs::remove_dir(&self.root);
        }
    }

    #[test]
    fn immutable_file_hashes_and_byte_ceilings_fail_closed() -> Result<()> {
        let mut scratch = Scratch::new()?;
        let path = scratch.file("fixture", b"exact bytes")?;
        let digest = sha256(b"exact bytes");
        assert_eq!(pinned(&path, &digest, 11)?, b"exact bytes");
        assert!(pinned(&path, &"0".repeat(64), 11).is_err());
        assert!(read(&path, 10).is_err());
        assert!(hash(&path, 10).is_err());
        assert!(write_new(&path, b"overwrite").is_err());
        let empty = scratch.file("empty", b"")?;
        assert!(hash(&empty, 11).is_err());
        assert!(plain(&scratch.root, false).is_err());
        assert!(plain(&path, true).is_err());
        Ok(())
    }

    #[test]
    fn archive_member_equivalence_rejects_changed_missing_and_unsafe_entries() -> Result<()> {
        let mut scratch = Scratch::new()?;
        let payload = scratch.file("payload", b"published runtime member")?;
        let asset = hash(&payload, 1024)?;
        let valid = scratch.archive("good.zip", "payload", b"published runtime member")?;
        verify_archive(&valid, std::slice::from_ref(&asset))?;
        let changed = scratch.archive("changed.zip", "payload", b"changed runtime member")?;
        assert!(verify_archive(&changed, std::slice::from_ref(&asset)).is_err());
        assert!(verify_archive(&valid, &[]).is_err());
        let traversal = scratch.archive("unsafe.zip", "../payload", b"published runtime member")?;
        assert!(verify_archive(&traversal, std::slice::from_ref(&asset)).is_err());
        let missing = scratch.archive("missing.zip", "invented", b"published runtime member")?;
        assert!(verify_archive(&missing, std::slice::from_ref(&asset)).is_err());
        Ok(())
    }
}
