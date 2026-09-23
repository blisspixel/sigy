mod metrics;
mod records;
mod scoring;
mod selection;

#[cfg(test)]
mod tests;

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;

use serde::Serialize;

use selection::{Partition, Selection, is_sha256, sha256};

const MAX_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = 512;
const USAGE: &str = "usage: scorer-probe selection MANIFEST calibration|holdout\n       scorer-probe score MANIFEST calibration|holdout REFERENCES REFERENCES_SHA256 CANDIDATES CANDIDATES_SHA256\nAll files are local JSON. Selection mappings and score output are evaluator-only.";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("scorer-probe: {}", diagnostic(&error));
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    match arguments.as_slice() {
        [command, manifest, partition] if command == "selection" => {
            let selection = Selection::from_frozen_bytes(&read_bounded(Path::new(manifest))?)?;
            let partition = Partition::parse(partition.to_str().ok_or("partition is not UTF-8")?)?;
            output(&selection.inventory(partition))
        }
        [
            command,
            manifest,
            partition,
            references,
            reference_digest,
            candidates,
            candidate_digest,
        ] if command == "score" => {
            let selection = Selection::from_frozen_bytes(&read_bounded(Path::new(manifest))?)?;
            let partition = Partition::parse(partition.to_str().ok_or("partition is not UTF-8")?)?;
            let reference_digest = reference_digest
                .to_str()
                .ok_or("reference digest is not UTF-8")?;
            let candidate_digest = candidate_digest
                .to_str()
                .ok_or("candidate digest is not UTF-8")?;
            let references = read_pinned(Path::new(references), reference_digest)?;
            let candidates = read_pinned(Path::new(candidates), candidate_digest)?;
            let validated = records::validate(
                &selection,
                partition,
                parse_references(&references)?,
                parse_candidates(&candidates)?,
            )?;
            output(&scoring::score(
                &selection,
                partition,
                validated,
                reference_digest.into(),
                candidate_digest.into(),
            )?)
        }
        _ => Err(USAGE.into()),
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|_| "cannot open input file")?;
    if !file
        .metadata()
        .map_err(|_| "cannot read input file metadata")?
        .is_file()
    {
        return Err("input must be a regular local file".into());
    }
    read_limited(file)
}

fn read_limited(reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_ARTIFACT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read input file")?;
    if bytes.len() as u64 > MAX_ARTIFACT_BYTES {
        return Err("input exceeds 2 MiB limit".into());
    }
    Ok(bytes)
}

fn read_pinned(path: &Path, digest: &str) -> Result<Vec<u8>, String> {
    if !is_sha256(digest) {
        return Err("expected digest must be 64 lowercase SHA-256 hex characters".into());
    }
    let bytes = read_bounded(path)?;
    verify_digest(&bytes, digest)?;
    Ok(bytes)
}

fn verify_digest(bytes: &[u8], digest: &str) -> Result<(), String> {
    if !is_sha256(digest) || sha256(bytes) != digest {
        return Err("input bytes do not match supplied immutable artifact digest".into());
    }
    Ok(())
}

fn output(value: &impl Serialize) -> Result<(), String> {
    let mut output = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut output, value).map_err(|_| "cannot write output JSON")?;
    writeln!(output).map_err(|_| "cannot finish output JSON".to_owned())
}

fn parse_references(bytes: &[u8]) -> Result<records::References, String> {
    serde_json::from_slice(bytes).map_err(|_| "invalid reference JSON".to_owned())
}

fn parse_candidates(bytes: &[u8]) -> Result<records::Candidates, String> {
    serde_json::from_slice(bytes).map_err(|_| "invalid candidate JSON".to_owned())
}

fn diagnostic(message: &str) -> String {
    message
        .chars()
        .flat_map(char::escape_default)
        .take(MAX_DIAGNOSTIC_BYTES)
        .collect()
}
