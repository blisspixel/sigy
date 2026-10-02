//! Frozen request manifests.
//!
//! A plan validates every input against the frozen FLEURS calibration
//! selection, builds each exact request body, computes its exact worst-case
//! reservation and hard bound from a hashed price snapshot, and writes the
//! bodies, an evaluator-only item list and a manifest into a new directory.
//! Only calibration-partition FLEURS text, constructed controls derived from it,
//! existing local outputs for those items, and calibration audio files whose
//! names map to the frozen selection can enter a request.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use base64::Engine;
use scorer_probe::{
    judge_records::{CalibrationControl, CalibrationSet},
    judge_scoring::BlindedInput,
    selection::{MANIFEST_SHA256, Partition, Selection, is_sha256, sha256},
    translation_records::{Candidates, InputOrigin, Outcome, References},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};

use crate::{
    Result, files, money,
    pricing::{self, Bounds, Rates, RoutePrices},
};

pub const RUBRIC: &str = include_str!("../../local-mt/judge-runner/rubric.txt");
pub const SCHEMA: &str = include_str!("../../local-mt/judge-runner/output-schema.json");
/// The local screen's raw template, without the local model's turn tokens.
pub const JUDGE_TEMPLATE: &str = "{rubric}\n\nDATA JSON:\n{data}";
/// The instruction used for both local translation calibration models.
pub const TRANSLATION_INSTRUCTION: &str = "Translate the following text into English. Note that you should only output the translated result without any additional explanation:";
pub const TRANSCRIPTION_INSTRUCTION: &str = "Transcribe the speech in this audio recording exactly as spoken, in its original language and writing system. Do not translate, summarize, correct or add anything. Output only the transcript text, without labels, timestamps, quotation marks or commentary. If the recording contains no intelligible speech, output nothing.";
/// Tokens allowed beyond one token per request-body byte for provider templates.
pub const PROMPT_MARGIN_TOKENS: u64 = 4096;
pub const HEADROOM_PERCENT: u32 = 5;
const MAX_AUDIO_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    JudgeControls,
    JudgeOutputs,
    Translate,
    Recognize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub schema_version: u32,
    pub label: String,
    pub model: String,
    pub vendor: String,
    pub endpoints_sha256: String,
    pub only: Vec<String>,
    pub zdr: bool,
    pub max_tokens: u64,
    pub max_tokens_includes_reasoning: bool,
    pub reasoning: Option<Value>,
    pub temperature_zero: bool,
}

fn token(text: &str, extra: &str, limit: usize) -> bool {
    !text.is_empty()
        && text.len() <= limit
        && text.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || extra.contains(character)
        })
}

impl Route {
    pub fn validate(&self) -> Result<()> {
        let vendor_matches = self
            .model
            .split_once('/')
            .is_some_and(|(vendor, _)| vendor == self.vendor);
        let reasoning_valid = match &self.reasoning {
            None => true,
            Some(Value::Object(object)) => object.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "effort" | "max_tokens" | "exclude" | "enabled"
                )
            }),
            Some(_) => false,
        };
        if self.schema_version != 1
            || !token(&self.label, "-.", 64)
            || !token(&self.model, "-./_", 128)
            || !vendor_matches
            || !is_sha256(&self.endpoints_sha256)
            || self.only.is_empty()
            || self.only.len() > 8
            || !self.only.iter().all(|slug| token(slug, "-./_", 64))
            || !(1..=16_384).contains(&self.max_tokens)
            || !reasoning_valid
        {
            return Err("route profile is invalid".into());
        }
        Ok(())
    }
}

/// Evaluator-only description of one planned item.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub id: String,
    pub clip_id: String,
    pub config: String,
    pub group: String,
    pub source_text: String,
    pub english_reference: String,
    pub output_text: String,
    pub untrusted_context: String,
    pub audio_sha256: Option<String>,
    pub audio_data_bytes: Option<u64>,
    pub requested: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Planned {
    pub index: u32,
    pub item_id: String,
    pub body_sha256: String,
    pub body_bytes: u64,
    pub bounds: Bounds,
    pub hard_bounds: Bounds,
    pub rates: Rates,
    pub reservation_micro: u64,
    pub hard_bound_micro: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub task: Task,
    pub route: Route,
    pub catalog_sha256: String,
    pub inputs: BTreeMap<String, String>,
    pub items_sha256: String,
    pub instruction_sha256: String,
    pub prompt_margin_tokens: u64,
    pub headroom_percent: u32,
    pub max_price_prompt_per_million: String,
    pub max_price_completion_per_million: String,
    pub matched_endpoints: Vec<String>,
    pub requests: Vec<Planned>,
    pub sum_reservation_micro: u64,
    pub max_reservation_micro: u64,
    pub sum_hard_bound_micro: u64,
    pub data_policy: String,
}

impl Manifest {
    pub fn load(directory: &Path, digest: &str) -> Result<(Self, Vec<Item>)> {
        let bytes = files::pinned(&directory.join("manifest.json"), digest, 4 * 1024 * 1024)?;
        let manifest: Self = serde_json::from_slice(&bytes)?;
        let items = files::pinned(
            &directory.join("items.json"),
            &manifest.items_sha256,
            8 * 1024 * 1024,
        )?;
        manifest.route.validate()?;
        Ok((manifest, serde_json::from_slice(&items)?))
    }

    #[must_use]
    pub fn body_path(directory: &Path, index: u32) -> PathBuf {
        directory.join("requests").join(format!("{index:04}.json"))
    }
}

#[derive(Serialize)]
struct Body<'a> {
    model: &'a str,
    messages: [Message; 1],
    max_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<Value>,
    provider: Provider<'a>,
    stream: bool,
}

#[derive(Serialize)]
struct Message {
    role: &'static str,
    content: Value,
}

#[derive(Serialize)]
struct Provider<'a> {
    only: &'a [String],
    allow_fallbacks: bool,
    require_parameters: bool,
    data_collection: &'static str,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    zdr: bool,
    max_price: MaxPrice,
}

#[derive(Serialize)]
struct MaxPrice {
    prompt: Box<RawValue>,
    completion: Box<RawValue>,
}

/// One item with the user-message content its request carries, if any.
#[derive(Debug)]
pub struct Draft {
    pub item: Item,
    pub content: Option<Value>,
    pub structured: bool,
}

#[derive(Debug)]
pub struct Inputs<'a> {
    pub task: Task,
    pub route: &'a Route,
    pub catalog: &'a [u8],
    pub endpoints: &'a [u8],
    pub digests: BTreeMap<String, String>,
    pub instruction: &'a str,
}

fn schema_format() -> Result<Value> {
    Ok(serde_json::json!({
        "type": "json_schema",
        "json_schema": {"name": "judgment", "strict": true, "schema": serde_json::from_str::<Value>(SCHEMA)?}
    }))
}

fn catalog_lists(catalog: &[u8], model: &str) -> Result<()> {
    let document: Value = serde_json::from_slice(catalog)?;
    let listed = document
        .get("data")
        .and_then(Value::as_array)
        .is_some_and(|models| {
            models
                .iter()
                .any(|entry| entry.get("id").and_then(Value::as_str) == Some(model))
        });
    if listed {
        Ok(())
    } else {
        Err("model is absent from the catalog price snapshot".into())
    }
}

fn body(
    route: &Route,
    content: Value,
    structured: bool,
    max_price: (&str, &str),
) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&Body {
        model: &route.model,
        messages: [Message {
            role: "user",
            content,
        }],
        max_tokens: route.max_tokens,
        reasoning: route.reasoning.as_ref(),
        temperature: route.temperature_zero.then_some(0),
        response_format: if structured {
            Some(schema_format()?)
        } else {
            None
        },
        provider: Provider {
            only: &route.only,
            allow_fallbacks: false,
            require_parameters: true,
            data_collection: "deny",
            zdr: route.zdr,
            max_price: MaxPrice {
                prompt: RawValue::from_string(max_price.0.to_owned())?,
                completion: RawValue::from_string(max_price.1.to_owned())?,
            },
        },
        stream: false,
    })?)
}

/// Exact bounds, rates, reservation and hard bound for one request body.
fn price(
    route: &Route,
    prices: &RoutePrices,
    index: u32,
    item: &Item,
    body: &[u8],
) -> Result<Planned> {
    let body_bytes = u64::try_from(body.len())?;
    let prompt_tokens = body_bytes
        .checked_add(PROMPT_MARGIN_TOKENS)
        .ok_or("prompt bound overflow")?
        .min(prices.context_length());
    let bounds = Bounds {
        prompt_tokens,
        completion_tokens: if route.max_tokens_includes_reasoning {
            route.max_tokens
        } else {
            prices.max_completion_tokens()
        },
        images: 0,
        web_searches: 0,
    };
    let hard_bounds = Bounds {
        completion_tokens: prices.max_completion_tokens(),
        web_searches: 1,
        ..bounds
    };
    let rates = prices
        .worst(prompt_tokens)
        .with_headroom(HEADROOM_PERCENT)?;
    Ok(Planned {
        index,
        item_id: item.id.clone(),
        body_sha256: sha256(body),
        body_bytes,
        bounds,
        hard_bounds,
        rates,
        reservation_micro: money::ceil_micro(pricing::liability(rates, bounds)?)?,
        hard_bound_micro: money::ceil_micro(pricing::liability(rates, hard_bounds)?)?,
    })
}

/// Build bodies, bounds and reservations, then write a new plan directory.
pub fn write(
    inputs: &Inputs<'_>,
    drafts: Vec<Draft>,
    directory: &Path,
) -> Result<(Manifest, String)> {
    let route = inputs.route;
    route.validate()?;
    if sha256(inputs.endpoints) != route.endpoints_sha256 {
        return Err("endpoint snapshot differs from the route's pinned hash".into());
    }
    catalog_lists(inputs.catalog, &route.model)?;
    let prices = RoutePrices::parse(inputs.endpoints, &route.model, &route.only)?;
    if route.max_tokens > prices.max_completion_tokens() {
        return Err("route output limit exceeds an endpoint ceiling".into());
    }
    let (listed_prompt, listed_completion) = prices.listed_max();
    let max_prompt = money::per_million(money::with_headroom(listed_prompt, HEADROOM_PERCENT)?)?;
    let max_completion =
        money::per_million(money::with_headroom(listed_completion, HEADROOM_PERCENT)?)?;
    std::fs::create_dir(directory)?;
    std::fs::create_dir(directory.join("requests"))?;
    let mut items = Vec::new();
    let mut requests = Vec::new();
    for draft in drafts {
        if let Some(content) = draft.content {
            let body = body(
                route,
                content,
                draft.structured,
                (&max_prompt, &max_completion),
            )?;
            let index = u32::try_from(requests.len())?;
            files::write_new(&Manifest::body_path(directory, index), &body)?;
            requests.push(price(route, &prices, index, &draft.item, &body)?);
        }
        items.push(draft.item);
    }
    if requests.is_empty() {
        return Err("plan contains no request".into());
    }
    let item_bytes = serde_json::to_vec_pretty(&items)?;
    files::write_new(&directory.join("items.json"), &item_bytes)?;
    let manifest = Manifest {
        schema_version: 1,
        task: inputs.task,
        route: route.clone(),
        catalog_sha256: sha256(inputs.catalog),
        inputs: inputs.digests.clone(),
        items_sha256: sha256(&item_bytes),
        instruction_sha256: sha256(inputs.instruction.as_bytes()),
        prompt_margin_tokens: PROMPT_MARGIN_TOKENS,
        headroom_percent: HEADROOM_PERCENT,
        max_price_prompt_per_million: max_prompt,
        max_price_completion_per_million: max_completion,
        matched_endpoints: prices.endpoints.iter().map(|endpoint| format!("{} ({})", endpoint.tag, endpoint.provider)).collect(),
        sum_reservation_micro: requests.iter().map(|request| request.reservation_micro).sum(),
        max_reservation_micro: requests.iter().map(|request| request.reservation_micro).max().unwrap_or(0),
        sum_hard_bound_micro: requests.iter().map(|request| request.hard_bound_micro).sum(),
        requests,
        data_policy: "Public FLEURS calibration-partition text and audio, constructed controls derived from it, and existing local model outputs for those items only. No user recordings, station captures or holdout material. Provider routing: listed providers only, no fallbacks, required parameters, data_collection deny, ZDR when the route sets it.".into(),
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    files::write_new(&directory.join("manifest.json"), &manifest_bytes)?;
    Ok((manifest, sha256(&manifest_bytes)))
}

fn judge_prompt(blinded: &BlindedInput<'_>) -> Result<String> {
    let data = serde_json::to_string(blinded)?;
    // Replace only the two fixed placeholders, as the local runner did.
    Ok(JUDGE_TEMPLATE
        .replacen("{rubric}", RUBRIC, 1)
        .replacen("{data}", &data, 1))
}

fn route_order(route: &Route, drafts: &mut [Draft]) {
    drafts.sort_by_cached_key(|draft| {
        sha256(format!("{}:{}", route.label, draft.item.id).as_bytes())
    });
}

/// The 126 frozen constructed controls, ordered per route by an opaque hash.
pub fn judge_controls(
    selection: &Selection,
    controls: &CalibrationSet,
    route: &Route,
) -> Result<Vec<Draft>> {
    scorer_probe::judge_records::validate(selection, controls)?;
    if controls.rubric_sha256 != sha256(RUBRIC.as_bytes()) || controls.controls.len() != 126 {
        return Err("controls do not carry the frozen rubric and 126 controls".into());
    }
    let mut drafts = Vec::new();
    for control in &controls.controls {
        let config = selection
            .asset(&control.clip_id)
            .ok_or("control clip is not frozen")?
            .config
            .clone();
        let prompt = judge_prompt(&BlindedInput {
            control_id: &control.control_id,
            source_text: &control.source_text,
            english_reference: &control.english_reference,
            output_text: &control.output_text,
            untrusted_context: &control.untrusted_context,
        })?;
        drafts.push(Draft {
            item: Item {
                id: control.control_id.clone(),
                clip_id: control.clip_id.clone(),
                config,
                group: format!("{:?}", control.category),
                source_text: control.source_text.clone(),
                english_reference: control.english_reference.clone(),
                output_text: control.output_text.clone(),
                untrusted_context: control.untrusted_context.clone(),
                audio_sha256: None,
                audio_data_bytes: None,
                requested: true,
            },
            content: Some(Value::String(prompt)),
            structured: true,
        });
    }
    route_order(route, &mut drafts);
    Ok(drafts)
}

fn references_by_clip(
    selection: &Selection,
    references: &References,
) -> Result<BTreeMap<String, scorer_probe::translation_records::Reference>> {
    if references.schema_version != 1
        || references.manifest_sha256 != MANIFEST_SHA256
        || references.partition != Partition::Calibration
        || references.records.len() != 28
    {
        return Err("exactly 28 frozen calibration references are required".into());
    }
    let mut map = BTreeMap::new();
    for reference in &references.records {
        scorer_probe::translation_records::validate_reference(
            selection,
            Partition::Calibration,
            reference,
        )?;
        if map
            .insert(reference.clip_id.clone(), reference.clone())
            .is_some()
        {
            return Err("duplicate reference".into());
        }
    }
    Ok(map)
}

/// Judge existing translations against published sources and references.
/// Item IDs are opaque hashes, so the judge cannot see which system produced
/// an output or which clip it came from.
pub fn judge_outputs(
    selection: &Selection,
    references: &References,
    runs: &[(String, Candidates)],
    route: &Route,
) -> Result<Vec<Draft>> {
    let by_clip = references_by_clip(selection, references)?;
    let mut drafts = Vec::new();
    for (label, candidates) in runs {
        if !token(label, "-.", 64) {
            return Err("run label is invalid".into());
        }
        scorer_probe::translation_records::validate(
            selection,
            Partition::Calibration,
            references.clone(),
            candidates.clone(),
        )?;
        for candidate in &candidates.records {
            let Outcome::Translated { text } = &candidate.outcome else {
                continue;
            };
            let reference = by_clip
                .get(&candidate.clip_id)
                .ok_or("candidate without reference")?;
            let item_id = sha256(
                format!("sigy-hosted-eval-output-v1:{label}:{}", candidate.clip_id).as_bytes(),
            );
            let prompt = judge_prompt(&BlindedInput {
                control_id: &item_id,
                source_text: &reference.source_text,
                english_reference: &reference.english_text,
                output_text: text,
                untrusted_context: "",
            })?;
            drafts.push(Draft {
                item: Item {
                    id: item_id,
                    clip_id: candidate.clip_id.clone(),
                    config: selection
                        .asset(&candidate.clip_id)
                        .ok_or("unknown clip")?
                        .config
                        .clone(),
                    group: label.clone(),
                    source_text: reference.source_text.clone(),
                    english_reference: reference.english_text.clone(),
                    output_text: text.clone(),
                    untrusted_context: String::new(),
                    audio_sha256: None,
                    audio_data_bytes: None,
                    requested: true,
                },
                content: Some(Value::String(prompt)),
                structured: true,
            });
        }
    }
    route_order(route, &mut drafts);
    Ok(drafts)
}

/// Hosted translation of published source text, or of the recorded local
/// recognizer text when `recognized` supplies that existing envelope.
pub fn translate(
    selection: &Selection,
    references: &References,
    recognized: Option<&Candidates>,
) -> Result<Vec<Draft>> {
    let by_clip = references_by_clip(selection, references)?;
    let inputs: BTreeMap<String, String> = match recognized {
        None => by_clip
            .iter()
            .map(|(clip, reference)| (clip.clone(), reference.source_text.clone()))
            .collect(),
        Some(candidates) => {
            scorer_probe::translation_records::validate(
                selection,
                Partition::Calibration,
                references.clone(),
                candidates.clone(),
            )?;
            if !matches!(candidates.input_origin, InputOrigin::RecognizedText { .. }) {
                return Err("recognized-text translation needs a recognizer envelope".into());
            }
            candidates
                .records
                .iter()
                .map(|record| (record.clip_id.clone(), record.input_text.clone()))
                .collect()
        }
    };
    let group = if recognized.is_some() {
        "recognized_text"
    } else {
        "reference_text"
    };
    let mut drafts = Vec::new();
    for (clip_id, input) in inputs {
        let requested = !input.trim().is_empty();
        drafts.push(Draft {
            item: Item {
                id: sha256(format!("sigy-hosted-eval-translate-v1:{group}:{clip_id}").as_bytes()),
                config: selection
                    .asset(&clip_id)
                    .ok_or("unknown clip")?
                    .config
                    .clone(),
                clip_id,
                group: group.into(),
                source_text: input.clone(),
                english_reference: String::new(),
                output_text: String::new(),
                untrusted_context: String::new(),
                audio_sha256: None,
                audio_data_bytes: None,
                requested,
            },
            content: requested
                .then(|| Value::String(format!("{TRANSLATION_INSTRUCTION}\n\n{input}"))),
            structured: false,
        });
    }
    Ok(drafts)
}

/// One contract probe per route, excluded from every score: a synthetic
/// date-change judgment, a synthetic sentence to translate, or the first
/// English calibration clip to transcribe.
pub fn probe(
    mode: &str,
    selection: &Selection,
    audio: &Path,
) -> Result<(Task, &'static str, Draft)> {
    let item = Item {
        id: sha256(format!("sigy-hosted-eval-probe-v1:{mode}").as_bytes()),
        clip_id: "synthetic-probe".into(),
        config: "fr_fr".into(),
        group: "probe".into(),
        source_text: "L'année est 2020.".into(),
        english_reference: "The year is 2020.".into(),
        output_text: "The year is 2030.".into(),
        untrusted_context: String::new(),
        audio_sha256: None,
        audio_data_bytes: None,
        requested: true,
    };
    match mode {
        "judge" => {
            let prompt = judge_prompt(&BlindedInput {
                control_id: &item.id,
                source_text: &item.source_text,
                english_reference: &item.english_reference,
                output_text: &item.output_text,
                untrusted_context: "",
            })?;
            Ok((
                Task::JudgeOutputs,
                RUBRIC,
                Draft {
                    item,
                    content: Some(Value::String(prompt)),
                    structured: true,
                },
            ))
        }
        "translate" => {
            let content = format!("{TRANSLATION_INSTRUCTION}\n\n{}", item.source_text);
            Ok((
                Task::Translate,
                TRANSLATION_INSTRUCTION,
                Draft {
                    item,
                    content: Some(Value::String(content)),
                    structured: false,
                },
            ))
        }
        "recognize" => {
            let first = recognize(selection, audio, &["en_us".into()])?
                .into_iter()
                .next()
                .ok_or("no English calibration clip")?;
            Ok((Task::Recognize, TRANSCRIPTION_INSTRUCTION, first))
        }
        _ => Err("probe mode must be judge, translate or recognize".into()),
    }
}

/// Return the PCM data length of a 16 kHz mono 16-bit PCM WAV file.
fn pcm_data_bytes(bytes: &[u8]) -> Result<u64> {
    let field = |start: usize, length: usize| {
        bytes
            .get(start..start + length)
            .ok_or("WAV header truncated")
    };
    let le16 =
        |start: usize| -> Result<u16> { Ok(u16::from_le_bytes(field(start, 2)?.try_into()?)) };
    let le32 =
        |start: usize| -> Result<u32> { Ok(u32::from_le_bytes(field(start, 4)?.try_into()?)) };
    if field(0, 4)? != b"RIFF" || field(8, 4)? != b"WAVE" {
        return Err("audio is not a RIFF WAVE file".into());
    }
    let mut offset = 12;
    let mut format_ok = false;
    while offset + 8 <= bytes.len() {
        let id = field(offset, 4)?;
        let size = usize::try_from(le32(offset + 4)?)?;
        let body = offset + 8;
        if id == b"fmt " {
            format_ok = le16(body)? == 1
                && le16(body + 2)? == 1
                && le32(body + 4)? == 16_000
                && le16(body + 14)? == 16;
        } else if id == b"data" {
            if !format_ok || body + size > bytes.len() || size == 0 {
                return Err("audio must be non-empty 16 kHz mono 16-bit PCM".into());
            }
            return Ok(u64::try_from(size)?);
        }
        offset = body + size + (size % 2);
    }
    Err("WAV data chunk missing".into())
}

/// Calibration clips decoded to 16 kHz mono PCM WAV, named `<config>-<id>.wav`.
/// Only files that map to the frozen calibration selection are read.
pub fn recognize(selection: &Selection, audio: &Path, configs: &[String]) -> Result<Vec<Draft>> {
    let mut drafts = Vec::new();
    for clip in selection.inventory(Partition::Calibration).clips {
        if !configs.iter().any(|config| config == clip.config) {
            continue;
        }
        let stem = clip
            .asset_id
            .strip_prefix(&format!("{}/train/", clip.config))
            .and_then(|name| name.strip_suffix(".wav"))
            .ok_or("unexpected calibration asset name")?;
        let bytes = files::read(
            &audio.join(format!("{}-{stem}.wav", clip.config)),
            MAX_AUDIO_BYTES,
        )?;
        let data_bytes = pcm_data_bytes(&bytes)?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        drafts.push(Draft {
            item: Item {
                id: sha256(format!("sigy-hosted-eval-recognize-v1:{}", clip.clip_id).as_bytes()),
                clip_id: clip.clip_id.into(),
                config: clip.config.into(),
                group: "calibration_audio".into(),
                source_text: String::new(),
                english_reference: String::new(),
                output_text: String::new(),
                untrusted_context: String::new(),
                audio_sha256: Some(sha256(&bytes)),
                audio_data_bytes: Some(data_bytes),
                requested: true,
            },
            content: Some(serde_json::json!([
                {"type": "text", "text": TRANSCRIPTION_INSTRUCTION},
                {"type": "input_audio", "input_audio": {"data": encoded, "format": "wav"}}
            ])),
            structured: false,
        });
    }
    if drafts.is_empty() {
        return Err("no calibration clip matched the requested languages".into());
    }
    Ok(drafts)
}

/// Rebuild the control a judge saw, for the frozen response parser.
#[must_use]
pub fn parser_control(item: &Item) -> CalibrationControl {
    CalibrationControl {
        control_id: item.id.clone(),
        clip_id: item.clip_id.clone(),
        category: scorer_probe::judge_records::Category::Unchanged,
        source_text: item.source_text.clone(),
        english_reference: item.english_reference.clone(),
        output_text: item.output_text.clone(),
        untrusted_context: item.untrusted_context.clone(),
        label_basis: scorer_probe::judge_records::LabelBasis::Unchanged {},
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::ledger::tests::scratch;

    const MANIFEST: &[u8] = include_bytes!("../../language-corpus/fleurs-screening-manifest.json");

    /// The frozen selection hash covers CRLF bytes, while Git stores the file
    /// with LF endings. Restore CRLF when a checkout produced LF.
    pub(crate) fn frozen_selection() -> Result<Selection> {
        if let Ok(selection) = Selection::from_frozen_bytes(MANIFEST) {
            return Ok(selection);
        }
        let text = std::str::from_utf8(MANIFEST)?;
        Ok(Selection::from_frozen_bytes(
            text.replace('\n', "\r\n").as_bytes(),
        )?)
    }

    pub(crate) fn route() -> Route {
        Route {
            schema_version: 1,
            label: "fixture-route".into(),
            model: "vendor/model".into(),
            vendor: "vendor".into(),
            endpoints_sha256: sha256(&endpoints()),
            only: vec!["alpha".into()],
            zdr: true,
            max_tokens: 64,
            max_tokens_includes_reasoning: true,
            reasoning: Some(serde_json::json!({"effort": "low"})),
            temperature_zero: false,
        }
    }

    pub(crate) fn endpoints() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"data": {"id": "vendor/model", "endpoints": [
            {"tag": "alpha", "provider_name": "Fixture", "context_length": 100_000, "max_completion_tokens": 1000,
             "pricing": {"prompt": "0.000002", "completion": "0.00001", "web_search": "0.01"}}
        ]}}))
        .unwrap_or_default()
    }

    pub(crate) fn catalog() -> Vec<u8> {
        br#"{"data":[{"id":"vendor/model"}]}"#.to_vec()
    }

    pub(crate) fn draft(id: &str, structured: bool) -> Draft {
        Draft {
            item: Item {
                id: id.into(),
                clip_id: "clip".into(),
                config: "fr_fr".into(),
                group: "fixture".into(),
                source_text: "La date est 2020.".into(),
                english_reference: "The date is 2020.".into(),
                output_text: "The date is 2030.".into(),
                untrusted_context: String::new(),
                audio_sha256: None,
                audio_data_bytes: None,
                requested: true,
            },
            content: Some(Value::String(format!("synthetic prompt {id}"))),
            structured,
        }
    }

    #[test]
    fn plan_writes_exact_bodies_bounds_and_reservations() -> Result<()> {
        let directory = scratch("plan")?.join("plan");
        let route = route();
        let endpoints = endpoints();
        let catalog = catalog();
        let inputs = Inputs {
            task: Task::JudgeControls,
            route: &route,
            catalog: &catalog,
            endpoints: &endpoints,
            digests: BTreeMap::from([("fixture".into(), sha256(b"x"))]),
            instruction: RUBRIC,
        };
        let mut skipped = draft("skipped", false);
        skipped.content = None;
        skipped.item.requested = false;
        let (manifest, digest) = write(
            &inputs,
            vec![draft("a", true), skipped, draft("b", false)],
            &directory,
        )?;
        assert_eq!(manifest.requests.len(), 2);
        let (loaded, items) = Manifest::load(&directory, &digest)?;
        assert_eq!(loaded, manifest);
        assert_eq!(items.len(), 3);
        let first = &manifest.requests[0];
        let body = std::fs::read(Manifest::body_path(&directory, 0))?;
        assert_eq!(sha256(&body), first.body_sha256);
        let value: Value = serde_json::from_slice(&body)?;
        assert_eq!(value["provider"]["allow_fallbacks"], false);
        assert_eq!(value["provider"]["data_collection"], "deny");
        assert_eq!(value["provider"]["zdr"], true);
        assert_eq!(value["provider"]["require_parameters"], true);
        assert_eq!(value["response_format"]["json_schema"]["strict"], true);
        assert!(value.get("temperature").is_none());
        // 5% headroom over the listed USD 2 and USD 10 per million.
        assert_eq!(manifest.max_price_prompt_per_million, "2.1");
        assert_eq!(manifest.max_price_completion_per_million, "10.5");
        assert_eq!(
            first.bounds.prompt_tokens,
            first.body_bytes + PROMPT_MARGIN_TOKENS
        );
        assert_eq!(first.bounds.completion_tokens, 64);
        assert_eq!(first.hard_bounds.completion_tokens, 1000);
        let expected = (first.bounds.prompt_tokens * 2_100 + 64 * 10_500).div_ceil(1000);
        assert_eq!(first.reservation_micro, expected);
        let hard = (first.bounds.prompt_tokens * 2_100 + 1000 * 10_500).div_ceil(1000) + 10_500;
        assert_eq!(first.hard_bound_micro, hard);
        let second: Value =
            serde_json::from_slice(&std::fs::read(Manifest::body_path(&directory, 1))?)?;
        assert!(second.get("response_format").is_none());
        // A plan directory is never reused, and pinned identities are checked.
        assert!(write(&inputs, vec![draft("c", true)], &directory).is_err());
        assert!(Manifest::load(&directory, &sha256(b"other")).is_err());
        Ok(())
    }

    #[test]
    fn plan_refuses_mismatched_snapshots_routes_and_empty_work() -> Result<()> {
        let base = scratch("plan-refusals")?;
        let endpoints = endpoints();
        let catalog = catalog();
        let mut route = route();
        let attempt = |route: &Route, catalog: &[u8], drafts: Vec<Draft>, name: &str| {
            write(
                &Inputs {
                    task: Task::Translate,
                    route,
                    catalog,
                    endpoints: &endpoints,
                    digests: BTreeMap::new(),
                    instruction: TRANSLATION_INSTRUCTION,
                },
                drafts,
                &base.join(name),
            )
        };
        assert!(attempt(&route, b"{\"data\":[]}", vec![draft("a", false)], "catalog").is_err());
        let mut none = draft("a", false);
        none.content = None;
        assert!(attempt(&route, &catalog, vec![none], "empty").is_err());
        route.max_tokens = 5000;
        assert!(attempt(&route, &catalog, vec![draft("a", false)], "ceiling").is_err());
        route.max_tokens = 64;
        route.endpoints_sha256 = sha256(b"other");
        assert!(attempt(&route, &catalog, vec![draft("a", false)], "hash").is_err());
        let mut exclusive = super::tests::route();
        exclusive.max_tokens_includes_reasoning = false;
        let (manifest, _) = attempt(&exclusive, &catalog, vec![draft("a", false)], "exclusive")?;
        assert_eq!(manifest.requests[0].bounds.completion_tokens, 1000);
        Ok(())
    }

    #[test]
    fn route_validation_rejects_hostile_profiles() {
        let good = route();
        assert!(good.validate().is_ok());
        let mutations: [fn(&mut Route); 9] = [
            |route| route.schema_version = 2,
            |route| route.label = "Upper".into(),
            |route| route.model = "vendor/model space".into(),
            |route| route.vendor = "other".into(),
            |route| route.only.clear(),
            |route| route.only = vec!["../x y".into()],
            |route| route.max_tokens = 0,
            |route| route.reasoning = Some(serde_json::json!({"tools": true})),
            |route| route.reasoning = Some(serde_json::json!("low")),
        ];
        for mutate in mutations {
            let mut route = good.clone();
            mutate(&mut route);
            assert!(route.validate().is_err(), "{route:?}");
        }
    }

    fn wav(rate: u32, channels: u16, bits: u16, data: usize) -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WAVE".to_vec();
        bytes.extend_from_slice(b"LIST\x03\0\0\0abc\0");
        bytes.extend_from_slice(b"fmt \x10\0\0\0");
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&u32::try_from(data).unwrap_or(0).to_le_bytes());
        bytes.extend(std::iter::repeat_n(0_u8, data));
        bytes
    }

    #[test]
    fn audio_admission_requires_frozen_names_and_pcm_wav() -> Result<()> {
        assert_eq!(pcm_data_bytes(&wav(16_000, 1, 16, 64))?, 64);
        assert!(pcm_data_bytes(&wav(8_000, 1, 16, 64)).is_err());
        assert!(pcm_data_bytes(&wav(16_000, 2, 16, 64)).is_err());
        assert!(pcm_data_bytes(&wav(16_000, 1, 16, 0)).is_err());
        assert!(pcm_data_bytes(b"RIFF\0\0\0\0WAVX").is_err());
        assert!(pcm_data_bytes(b"RIFF").is_err());
        assert!(pcm_data_bytes(b"RIFF\0\0\0\0WAVE").is_err());
        let mut truncated = wav(16_000, 1, 16, 64);
        truncated.truncate(truncated.len() - 10);
        assert!(pcm_data_bytes(&truncated).is_err());
        let selection = frozen_selection()?;
        let audio = scratch("audio")?;
        let mut written = 0;
        for clip in selection.inventory(Partition::Calibration).clips {
            if clip.config == "sw_ke" {
                let stem = clip
                    .asset_id
                    .trim_start_matches("sw_ke/train/")
                    .trim_end_matches(".wav");
                std::fs::write(
                    audio.join(format!("sw_ke-{stem}.wav")),
                    wav(16_000, 1, 16, 320),
                )?;
                written += 1;
            }
        }
        std::fs::write(audio.join("control-silence.wav"), wav(16_000, 1, 16, 320))?;
        let drafts = recognize(&selection, &audio, &["sw_ke".into()])?;
        assert_eq!((drafts.len(), written), (4, 4));
        assert!(
            drafts
                .iter()
                .all(|draft| draft.item.audio_data_bytes == Some(320))
        );
        let content = drafts[0].content.clone().unwrap_or_default();
        assert_eq!(content[1]["input_audio"]["format"], "wav");
        assert!(recognize(&selection, &audio, &["hi_in".into()]).is_err());
        assert!(recognize(&selection, &audio, &["xx".into()]).is_err());
        Ok(())
    }

    #[test]
    fn probes_are_single_synthetic_or_calibration_requests() -> Result<()> {
        let selection = frozen_selection()?;
        let audio = scratch("probe-audio")?;
        let (task, _, judge) = probe("judge", &selection, &audio)?;
        assert_eq!(task, Task::JudgeOutputs);
        assert!(judge.structured);
        let critical = r#"{"status":"critical","source_quote":"2020","reference_quote":"2020","output_quote":"2030","reason":"Date changed."}"#;
        assert!(crate::response::parse(critical.as_bytes(), &parser_control(&judge.item)).is_ok());
        let (task, instruction, translate) = probe("translate", &selection, &audio)?;
        assert_eq!(
            (task, instruction),
            (Task::Translate, TRANSLATION_INSTRUCTION)
        );
        assert!(translate.content.is_some_and(|content| {
            content
                .as_str()
                .is_some_and(|text| text.ends_with("L'année est 2020."))
        }));
        assert!(probe("recognize", &selection, &audio).is_err());
        for clip in selection.inventory(Partition::Calibration).clips {
            if clip.config == "en_us" {
                let stem = clip
                    .asset_id
                    .trim_start_matches("en_us/train/")
                    .trim_end_matches(".wav");
                std::fs::write(
                    audio.join(format!("en_us-{stem}.wav")),
                    wav(16_000, 1, 16, 32),
                )?;
            }
        }
        let (task, _, recognize) = probe("recognize", &selection, &audio)?;
        assert_eq!(
            (task, recognize.item.config.as_str()),
            (Task::Recognize, "en_us")
        );
        assert!(probe("other", &selection, &audio).is_err());
        Ok(())
    }

    #[test]
    fn judge_prompt_matches_the_local_template_text() -> Result<()> {
        let prompt = judge_prompt(&BlindedInput {
            control_id: "id",
            source_text: "{data}",
            english_reference: "r",
            output_text: "o",
            untrusted_context: "",
        })?;
        assert!(prompt.starts_with(RUBRIC));
        assert!(prompt.ends_with("\n\nDATA JSON:\n{\"control_id\":\"id\",\"source_text\":\"{data}\",\"english_reference\":\"r\",\"output_text\":\"o\",\"untrusted_context\":\"\"}"));
        let control = parser_control(&draft("x", true).item);
        assert_eq!(control.output_text, "The date is 2030.");
        Ok(())
    }
}
