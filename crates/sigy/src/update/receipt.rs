use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_RECEIPT: u64 = 8192;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub protocol: u32,
    pub operation: String,
    pub state: String,
    pub commit: String,
    pub install_root: PathBuf,
    pub reason: Option<String>,
}

pub(super) fn path() -> Result<PathBuf, String> {
    Ok(super::home_dir()
        .map_err(str::to_owned)?
        .join(".sigy/update-status.json"))
}

pub(super) fn write(receipt: &Receipt) -> Result<(), String> {
    let bytes = serde_json::to_vec(receipt).map_err(|_| "cannot encode update receipt")?;
    if bytes.len() > usize::try_from(MAX_RECEIPT).map_err(|_| "update receipt bound")? {
        return Err("update receipt exceeds its byte bound".into());
    }
    let path = path()?;
    let temporary = path.with_extension(format!("json.tmp-{}", receipt.operation));
    let mut file = fs::File::create_new(&temporary).map_err(|_| "cannot write update receipt")?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "cannot persist update receipt")?;
    drop(file);
    fs::rename(temporary, path).map_err(|_| "cannot publish update receipt".into())
}

fn read(path: &Path) -> Result<Option<Receipt>, String> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("cannot inspect update receipt".into()),
    };
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_RECEIPT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read update receipt")?;
    if u64::try_from(bytes.len()).map_err(|_| "update receipt bound")? > MAX_RECEIPT {
        return Err("update receipt exceeds its byte bound".into());
    }
    let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|_| "invalid update receipt")?;
    if receipt.protocol != 1
        || !super::is_full_commit(&receipt.commit)
        || !matches!(
            receipt.state.as_str(),
            "pending" | "running" | "succeeded" | "failed"
        )
        || !valid_operation(&receipt.operation)
        || !receipt.install_root.is_absolute()
        || receipt.install_root.to_string_lossy().len() > 4096
        || receipt
            .reason
            .as_ref()
            .is_some_and(|reason| reason.len() > 256 || reason.chars().any(char::is_control))
    {
        return Err("invalid update receipt fields".into());
    }
    Ok(Some(receipt))
}

fn valid_operation(value: &str) -> bool {
    value.len() <= 80
        && value.split_once('-').is_some_and(|(process, stamp)| {
            !process.is_empty()
                && !stamp.is_empty()
                && process.bytes().all(|byte| byte.is_ascii_digit())
                && stamp.bytes().all(|byte| byte.is_ascii_digit())
        })
}

pub(super) fn status(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let receipt = read(&path()?)?;
    let mut output = std::io::stdout().lock();
    if json {
        serde_json::to_writer(
            &mut output,
            &serde_json::json!({
                "recorded_update": receipt, "liveness_observed": false
            }),
        )?;
        writeln!(output)?;
    } else if let Some(receipt) = receipt {
        writeln!(
            output,
            "Recorded update {}: {} at {}.",
            receipt.operation, receipt.state, receipt.commit
        )?;
        writeln!(
            output,
            "Install root: {}",
            crate::explorer::text::sanitize(&receipt.install_root.to_string_lossy(), 4096)
        )?;
        if let Some(reason) = receipt.reason {
            writeln!(
                output,
                "Reason: {}",
                crate::explorer::text::sanitize(&reason, 256)
            )?;
        }
        if matches!(receipt.state.as_str(), "pending" | "running") {
            writeln!(
                output,
                "Completion is unproven. A recorded pending state does not establish helper liveness."
            )?;
        }
    } else {
        writeln!(output, "No update outcome is recorded.")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_identity_is_bounded_and_terminal_safe() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("receipt");
        let mut receipt = Receipt {
            protocol: 1,
            operation: "12-34".into(),
            state: "succeeded".into(),
            commit: "a".repeat(40),
            install_root: directory.path().to_path_buf(),
            reason: None,
        };
        fs::write(&path, serde_json::to_vec(&receipt)?)?;
        assert!(read(&path)?.is_some());
        for operation in ["", "--", "1-2-3", "1-", "\u{1b}[31m"] {
            receipt.operation = operation.into();
            fs::write(&path, serde_json::to_vec(&receipt)?)?;
            assert!(read(&path).is_err());
        }
        receipt.operation = "12-34".into();
        receipt.reason = Some("unsafe\nreason".into());
        fs::write(&path, serde_json::to_vec(&receipt)?)?;
        assert!(read(&path).is_err());
        receipt.reason = None;
        receipt.install_root = PathBuf::from("relative");
        fs::write(&path, serde_json::to_vec(&receipt)?)?;
        assert!(read(&path).is_err());
        Ok(())
    }

    #[test]
    fn receipt_refuses_truncation_unknown_fields_and_oversize()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("receipt");
        assert!(read(&path)?.is_none());
        for bytes in [
            b"{".to_vec(),
            vec![b' '; 8193],
            b"{\"protocol\":1,\"extra\":true}".to_vec(),
        ] {
            fs::write(&path, bytes)?;
            assert!(read(&path).is_err());
        }
        Ok(())
    }
}
