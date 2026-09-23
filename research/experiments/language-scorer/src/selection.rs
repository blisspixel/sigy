use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MANIFEST_SHA256: &str =
    "487632ee83967f868afa6313d54f544a32cb07075d3b446701b05af870389fd9";
pub const DATASET_REVISION: &str = "70bb2e84b976b7e960aa89f1c648e09c59f894dd";
pub const CONFIGS: [&str; 8] = [
    "ar_eg",
    "cmn_hans_cn",
    "en_us",
    "es_419",
    "fr_fr",
    "hi_in",
    "pt_br",
    "sw_ke",
];
pub const TRAIN_GROUPS: [u32; 4] = [1087, 264, 773, 831];
pub const TEST_GROUPS: [u32; 10] = [1966, 1793, 1762, 1881, 1803, 1790, 1866, 1939, 1740, 1742];

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Partition {
    Calibration,
    Holdout,
}

impl Partition {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "calibration" => Ok(Self::Calibration),
            "holdout" => Ok(Self::Holdout),
            _ => Err("partition must be calibration or holdout".into()),
        }
    }

    pub const fn groups(self) -> &'static [u32] {
        match self {
            Self::Calibration => &TRAIN_GROUPS,
            Self::Holdout => &TEST_GROUPS,
        }
    }

    const fn split(self) -> &'static str {
        match self {
            Self::Calibration => "train",
            Self::Holdout => "test",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ManifestEntry {
    pub asset_id: String,
    pub config: String,
    pub partition: Partition,
    pub split: String,
    pub sentence_group_id: u32,
    pub raw_transcription_sha256: String,
}

#[derive(Deserialize)]
struct Manifest {
    schema_version: u32,
    dataset: String,
    revision: String,
    assets: Vec<ManifestEntry>,
}

#[derive(Clone)]
pub struct Selection {
    pub by_id: BTreeMap<String, ManifestEntry>,
}

#[derive(Serialize)]
pub struct Inventory<'a> {
    pub schema_version: u32,
    pub manifest_sha256: &'static str,
    pub dataset_revision: &'static str,
    pub partition: Partition,
    pub purpose: &'static str,
    pub clips: Vec<InventoryClip<'a>>,
}

#[derive(Serialize)]
pub struct InventoryClip<'a> {
    pub clip_id: &'a str,
    pub asset_id: &'a str,
    pub config: &'a str,
    pub reference_sha256: &'a str,
}

impl Selection {
    pub fn from_frozen_bytes(bytes: &[u8]) -> Result<Self, String> {
        if sha256(bytes) != MANIFEST_SHA256 {
            return Err("manifest bytes do not match the frozen 112-clip selection".into());
        }
        let manifest: Manifest =
            serde_json::from_slice(bytes).map_err(|_| "invalid manifest JSON")?;
        if manifest.schema_version != 1
            || manifest.dataset != "google/fleurs"
            || manifest.revision != DATASET_REVISION
        {
            return Err("unexpected manifest schema or dataset revision".into());
        }
        Self::from_assets(manifest.assets)
    }

    fn from_assets(assets: Vec<ManifestEntry>) -> Result<Self, String> {
        if assets.len() != 112 {
            return Err("selection must have exactly 112 assets".into());
        }
        let mut by_id = BTreeMap::new();
        let mut groups = BTreeSet::new();
        let mut partition_groups: BTreeMap<Partition, BTreeSet<u32>> = BTreeMap::new();
        for asset in assets {
            validate_asset(&asset)?;
            if !groups.insert((
                asset.config.clone(),
                asset.partition,
                asset.sentence_group_id,
            )) {
                return Err("duplicate language/partition/sentence group".into());
            }
            partition_groups
                .entry(asset.partition)
                .or_default()
                .insert(asset.sentence_group_id);
            let id = opaque_id(&asset.asset_id);
            if by_id.insert(id, asset).is_some() {
                return Err("duplicate asset or clip ID".into());
            }
        }
        let calibration = partition_groups
            .get(&Partition::Calibration)
            .ok_or("missing calibration partition")?;
        let holdout = partition_groups
            .get(&Partition::Holdout)
            .ok_or("missing holdout partition")?;
        if !calibration.is_disjoint(holdout) {
            return Err("sentence group leaks across calibration and holdout".into());
        }
        for config in CONFIGS {
            for partition in [Partition::Calibration, Partition::Holdout] {
                for group in partition.groups() {
                    if !groups.contains(&(config.to_owned(), partition, *group)) {
                        return Err(
                            "selection omits a frozen language/partition/sentence group".into()
                        );
                    }
                }
            }
        }
        Ok(Self { by_id })
    }

    pub fn inventory(&self, partition: Partition) -> Inventory<'_> {
        Inventory {
            schema_version: 1,
            manifest_sha256: MANIFEST_SHA256,
            dataset_revision: DATASET_REVISION,
            partition,
            purpose: "evaluator-only mapping; never expose references or this mapping to workers",
            clips: self
                .by_id
                .iter()
                .filter(|(_, asset)| asset.partition == partition)
                .map(|(id, asset)| InventoryClip {
                    clip_id: id,
                    asset_id: &asset.asset_id,
                    config: &asset.config,
                    reference_sha256: &asset.raw_transcription_sha256,
                })
                .collect(),
        }
    }
}

fn validate_asset(asset: &ManifestEntry) -> Result<(), String> {
    if !CONFIGS.contains(&asset.config.as_str())
        || asset.split != asset.partition.split()
        || !asset.partition.groups().contains(&asset.sentence_group_id)
        || !is_sha256(&asset.raw_transcription_sha256)
        || !asset
            .asset_id
            .starts_with(&format!("{}/{}/", asset.config, asset.split))
    {
        return Err("invalid frozen asset identity or partition".into());
    }
    Ok(())
}

pub fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(DIGITS[usize::from(byte >> 4)]),
                char::from(DIGITS[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

fn opaque_id(asset_id: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"sigy-fleurs-clip-v1\0");
    digest.update(asset_id.as_bytes());
    format!("clip-{}", hex(&digest.finalize()))
}

#[cfg(test)]
pub fn synthetic_selection() -> Result<Selection, String> {
    let mut assets = Vec::new();
    for config in CONFIGS {
        for partition in [Partition::Calibration, Partition::Holdout] {
            for group in partition.groups() {
                assets.push(ManifestEntry {
                    asset_id: format!("{config}/{}/{group}.wav", partition.split()),
                    config: config.into(),
                    partition,
                    split: partition.split().into(),
                    sentence_group_id: *group,
                    raw_transcription_sha256: sha256(b"synthetic reference"),
                });
            }
        }
    }
    Selection::from_assets(assets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_frozen_selection_has_exact_partition_counts() -> Result<(), String> {
        let selection = Selection::from_frozen_bytes(include_bytes!(
            "../../language-corpus/fleurs-screening-manifest.json"
        ))?;
        assert_eq!(selection.by_id.len(), 112);
        assert_eq!(selection.inventory(Partition::Calibration).clips.len(), 32);
        assert_eq!(selection.inventory(Partition::Holdout).clips.len(), 80);
        Ok(())
    }

    #[test]
    fn mutable_manifest_bytes_are_refused() {
        assert!(Selection::from_frozen_bytes(b"{}").is_err());
    }

    #[test]
    fn missing_duplicate_and_leaking_groups_are_refused() -> Result<(), String> {
        let selection = synthetic_selection()?;
        let original: Vec<_> = selection.by_id.into_values().collect();
        let mut missing = original.clone();
        missing.pop();
        assert!(Selection::from_assets(missing).is_err());
        let mut duplicate = original.clone();
        duplicate[1] = duplicate[0].clone();
        assert!(Selection::from_assets(duplicate).is_err());
        let mut leakage = original;
        let asset = leakage
            .iter_mut()
            .find(|asset| asset.partition == Partition::Holdout)
            .ok_or("test fixture has no holdout")?;
        asset.sentence_group_id = TRAIN_GROUPS[0];
        assert!(Selection::from_assets(leakage).is_err());
        Ok(())
    }
}
