use std::{
    collections::BTreeMap,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Map, Value, json};

const SERVER_VERSION: &str = "0.1.0-dev";
const MAX_TEXT: u64 = 2_048;
const MAX_OUTPUT: usize = 65_536;

#[derive(Clone, Copy)]
enum Field {
    Text,
    StationOrder,
    Count,
    Flag,
}

struct Slot {
    key: &'static str,
    kind: Field,
    flag: &'static str,
    required: bool,
    minimum: u64,
    maximum: u64,
    description: &'static str,
}

struct Tool {
    name: &'static str,
    description: &'static str,
    hints: u8,
    words: &'static [&'static str],
    slots: &'static [Slot],
    timeout: Duration,
}

pub(crate) fn tool_list() -> Value {
    json!({
        "resultType": "complete",
        "tools": TOOLS.iter().map(tool_schema).collect::<Vec<_>>(),
        "ttlMs": 300_000,
        "cacheScope": "public",
        "_meta": server_meta(),
    })
}

pub(crate) fn call(executable: &Path, directory: &Path, name: &str, arguments: &Value) -> Value {
    let Some(tool) = TOOLS.iter().find(|tool| tool.name == name) else {
        return error_result(format!("unknown tool: {name}"));
    };
    let Some(arguments) = arguments
        .as_object()
        .cloned()
        .or_else(|| arguments.is_null().then(Map::new))
    else {
        return error_result("tool arguments must be an object");
    };
    let args = match argv(tool, &arguments) {
        Ok(args) => args,
        Err(message) => return error_result(message),
    };
    match run(executable, directory, &args, tool.timeout) {
        Ok(output) => output,
        Err(message) => error_result(message),
    }
}

fn tool_schema(tool: &Tool) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for slot in tool.slots {
        properties.insert(
            slot.key.to_owned(),
            json!({
                "type": match slot.kind {
                    Field::Text | Field::StationOrder => "string",
                    Field::Count => "integer",
                    Field::Flag => "boolean",
                },
                "description": slot.description,
            }),
        );
        if let Some(property) = properties.get_mut(slot.key) {
            match slot.kind {
                Field::Text | Field::StationOrder => {
                    property["minLength"] = json!(1);
                    property["maxLength"] = json!(slot.maximum);
                    property["x-sigy-maxBytes"] = json!(slot.maximum);
                    if matches!(slot.kind, Field::StationOrder) {
                        property["enum"] = json!(["id", "name"]);
                    }
                }
                Field::Count => {
                    property["minimum"] = json!(slot.minimum);
                    property["maximum"] = json!(slot.maximum);
                }
                Field::Flag => {}
            }
        }
        if slot.required {
            required.push(slot.key);
        }
    }
    json!({
        "name": tool.name,
        "title": tool.name,
        "description": tool.description,
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "properties": properties,
            "required": required,
        },
        "annotations": {
            "readOnlyHint": (tool.hints & HINT_READ) != 0,
            "destructiveHint": (tool.hints & HINT_DESTRUCTIVE) != 0,
            "idempotentHint": (tool.hints & HINT_IDEMPOTENT) != 0,
            "openWorldHint": (tool.hints & HINT_WORLD) != 0,
        },
    })
}

fn argv(tool: &Tool, arguments: &Map<String, Value>) -> Result<Vec<String>, String> {
    let known: BTreeMap<&str, &Slot> = tool.slots.iter().map(|slot| (slot.key, slot)).collect();
    if let Some(key) = arguments
        .keys()
        .find(|key| !known.contains_key(key.as_str()))
    {
        return Err(format!("unexpected argument: {key}"));
    }
    let mut args: Vec<String> = tool.words.iter().map(|word| (*word).to_owned()).collect();
    for slot in tool.slots {
        match arguments.get(slot.key) {
            None if slot.required => return Err(format!("missing argument: {}", slot.key)),
            None => {}
            Some(value) => push_slot(&mut args, slot, value)?,
        }
    }
    Ok(args)
}

fn push_slot(args: &mut Vec<String>, slot: &Slot, value: &Value) -> Result<(), String> {
    match slot.kind {
        Field::Text | Field::StationOrder => {
            let text = value
                .as_str()
                .filter(|text| bounded(text, slot.maximum))
                .ok_or_else(|| format!("{} must be a short string", slot.key))?;
            if matches!(slot.kind, Field::StationOrder) && !matches!(text, "id" | "name") {
                return Err("order must be id or name".into());
            }
            if slot.flag.is_empty() {
                if text.starts_with('-') {
                    return Err(format!("{} cannot start with a hyphen", slot.key));
                }
                args.push(text.to_owned());
            } else {
                args.push(slot.flag.to_owned());
                args.push(text.to_owned());
            }
        }
        Field::Count => {
            let count = value
                .as_u64()
                .filter(|count| (slot.minimum..=slot.maximum).contains(count))
                .ok_or_else(|| {
                    format!(
                        "{} must be an integer from {} to {}",
                        slot.key, slot.minimum, slot.maximum
                    )
                })?;
            args.push(slot.flag.to_owned());
            args.push(count.to_string());
        }
        Field::Flag => {
            if value.as_bool() == Some(true) {
                args.push(slot.flag.to_owned());
            } else if value.as_bool() != Some(false) {
                return Err(format!("{} must be a boolean", slot.key));
            }
        }
    }
    Ok(())
}

fn bounded(text: &str, maximum: u64) -> bool {
    !text.is_empty()
        && text.len() <= usize::try_from(maximum).unwrap_or(0)
        && !text.chars().any(char::is_control)
}

fn run(
    executable: &Path,
    directory: &Path,
    args: &[String],
    timeout: Duration,
) -> Result<Value, String> {
    let mut child = Command::new(executable)
        .arg("--data-dir")
        .arg(directory)
        .arg("--json")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "could not start sigy".to_owned())?;
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let stdout_handle = thread::spawn(move || {
        let mut out = Vec::new();
        if let Some(mut pipe) = stdout_pipe.take() {
            let _ = pipe
                .by_ref()
                .take(u64::try_from(MAX_OUTPUT).unwrap_or(u64::MAX))
                .read_to_end(&mut out);
        }
        out
    });
    let stderr_handle = thread::spawn(move || {
        let mut err = Vec::new();
        if let Some(mut pipe) = stderr_pipe.take() {
            let _ = pipe
                .by_ref()
                .take(u64::try_from(MAX_OUTPUT).unwrap_or(u64::MAX))
                .read_to_end(&mut err);
        }
        err
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|_| "could not observe sigy".to_owned())?
        {
            break status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_handle.join();
            let _ = stderr_handle.join();
            return Err(format!(
                "tool exceeded {} seconds; if the service already admitted the job, it keeps running",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout_bytes = stdout_handle.join().unwrap_or_default();
    let stderr_bytes = stderr_handle.join().unwrap_or_default();
    let stdout = String::from_utf8_lossy(&stdout_bytes);
    let stderr = String::from_utf8_lossy(&stderr_bytes);
    if status.success() {
        Ok(success_result(stdout.trim()))
    } else {
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        Ok(error_result(if detail.is_empty() {
            "command failed".to_owned()
        } else {
            detail.to_owned()
        }))
    }
}

fn success_result(text: &str) -> Value {
    let mut result = json!({
        "resultType": "complete",
        "content": [{"type": "text", "text": text}],
        "isError": false,
        "_meta": server_meta(),
    });
    if let Ok(parsed) = serde_json::from_str::<Value>(text) {
        result["structuredContent"] = parsed;
    }
    result
}

fn error_result(message: impl Into<String>) -> Value {
    json!({
        "resultType": "complete",
        "content": [{"type": "text", "text": message.into()}],
        "isError": true,
        "_meta": server_meta(),
    })
}

pub(crate) fn server_meta() -> Value {
    json!({
        "io.modelcontextprotocol/serverInfo": {
            "name": "sigy",
            "version": SERVER_VERSION,
        }
    })
}

const HINT_READ: u8 = 0b0001;
const HINT_IDEMPOTENT: u8 = 0b0010;
const HINT_DESTRUCTIVE: u8 = 0b0100;
const HINT_WORLD: u8 = 0b1000;
const HINT_QUERY: u8 = HINT_READ | HINT_IDEMPOTENT;
const HINT_CHANGE: u8 = HINT_IDEMPOTENT;
const HINT_STOP: u8 = HINT_IDEMPOTENT | HINT_DESTRUCTIVE;
const HINT_NETWORK: u8 = HINT_IDEMPOTENT | HINT_WORLD;

const SHORT: Duration = Duration::from_secs(45);
const PLAY: Duration = Duration::from_secs(120);

const TOOLS: &[Tool] = &[
    Tool {
        name: "doctor",
        description: "Check the catalog, decoder, quota, and cache age. Does not refresh, delete, or use the network.",
        hints: HINT_QUERY,
        words: &["doctor"],
        slots: &[],
        timeout: SHORT,
    },
    Tool {
        name: "library_status",
        description: "Show the catalog state for the configured library. Does not create it.",
        hints: HINT_QUERY,
        words: &["library", "status"],
        slots: &[],
        timeout: SHORT,
    },
    Tool {
        name: "budget_show",
        description: "Show settled, reserved, and remaining USD. Does not enable a provider or change a limit.",
        hints: HINT_QUERY,
        words: &["budget", "show"],
        slots: &[],
        timeout: SHORT,
    },
    Tool {
        name: "service_status",
        description: "Show whether the local controller is running. Does not start or stop it.",
        hints: HINT_QUERY,
        words: &["service", "status"],
        slots: &[],
        timeout: SHORT,
    },
    Tool {
        name: "dvr_status",
        description: "Show retention days, quota, charged bytes, and reserved bytes.",
        hints: HINT_QUERY,
        words: &["dvr", "status"],
        slots: &[],
        timeout: SHORT,
    },
    Tool {
        name: "source_show",
        description: "Show one immutable source revision. Ordinary views omit the URL path and query.",
        hints: HINT_QUERY,
        words: &["source", "show"],
        slots: &[pos("revision", "Source revision id.")],
        timeout: SHORT,
    },
    Tool {
        name: "radio_status",
        description: "Show the local directory cache and the latest refresh. Does not contact a mirror.",
        hints: HINT_QUERY,
        words: &["radio", "status"],
        slots: &[],
        timeout: SHORT,
    },
    Tool {
        name: "radio_search",
        description: "Search the cached directory. This does not tune, record, or use the network.",
        hints: HINT_QUERY,
        words: &["radio", "search"],
        slots: RADIO_SEARCH,
        timeout: SHORT,
    },
    Tool {
        name: "radio_show",
        description: "Show one cached station. This does not tune or record.",
        hints: HINT_QUERY,
        words: &["radio", "show"],
        slots: &[pos("id", "Cached station id.")],
        timeout: SHORT,
    },
    Tool {
        name: "record_list",
        description: "List recordings, including failed and interrupted history.",
        hints: HINT_QUERY,
        words: &["record", "list"],
        slots: PAGE,
        timeout: SHORT,
    },
    Tool {
        name: "record_show",
        description: "Show one recording, including storage state and hash when published.",
        hints: HINT_QUERY,
        words: &["record", "show"],
        slots: &[pos("id", "Recording id.")],
        timeout: SHORT,
    },
    Tool {
        name: "record_path",
        description: "Return the local path of one verified retained recording.",
        hints: HINT_QUERY,
        words: &["record", "path"],
        slots: &[pos("id", "Recording id.")],
        timeout: SHORT,
    },
    Tool {
        name: "podcast_list",
        description: "List subscriptions. Feed paths and queries are omitted.",
        hints: HINT_QUERY,
        words: &["podcast", "list"],
        slots: PAGE,
        timeout: SHORT,
    },
    Tool {
        name: "podcast_show",
        description: "Show one subscription origin and network grant. The feed path is omitted.",
        hints: HINT_QUERY,
        words: &["podcast", "show"],
        slots: &[pos("id", "Subscription id.")],
        timeout: SHORT,
    },
    Tool {
        name: "podcast_episodes",
        description: "List stored episodes offline. Enclosure, transcript, and chapter URLs are omitted.",
        hints: HINT_QUERY,
        words: &["podcast", "episodes"],
        slots: EPISODES,
        timeout: SHORT,
    },
    Tool {
        name: "podcast_refresh_status",
        description: "Show one feed refresh. A failed document leaves the previous snapshot.",
        hints: HINT_QUERY,
        words: &["podcast", "refresh-status"],
        slots: &[pos("id", "Refresh id.")],
        timeout: SHORT,
    },
    Tool {
        name: "listen_status",
        description: "Show one direct-listen receipt. Retained playback uses separate reader receipts.",
        hints: HINT_QUERY,
        words: &["listen", "status"],
        slots: &[pos("id", "Listen id.")],
        timeout: SHORT,
    },
    Tool {
        name: "service_start",
        description: "Start the local controller if it is not already running. Client exit does not stop it.",
        hints: HINT_CHANGE,
        words: &["service", "start"],
        slots: &[],
        timeout: SHORT,
    },
    Tool {
        name: "service_stop",
        description: "Stop the local controller. This does not delete the library.",
        hints: HINT_STOP,
        words: &["service", "stop"],
        slots: &[],
        timeout: SHORT,
    },
    Tool {
        name: "podcast_subscribe",
        description: "Store one feed subscription. Does not resolve DNS, download, or record. The URL, pin, and redirects cannot change for this id.",
        hints: HINT_NETWORK,
        words: &["podcast", "subscribe"],
        slots: SUBSCRIBE,
        timeout: SHORT,
    },
    Tool {
        name: "podcast_unsubscribe",
        description: "Stop future polls for one subscription. Deletes nothing.",
        hints: HINT_CHANGE,
        words: &["podcast", "unsubscribe"],
        slots: &[pos("id", "Subscription id.")],
        timeout: SHORT,
    },
    Tool {
        name: "podcast_refresh",
        description: "Fetch one RSS 2.0 document on the shared acquirer. Does not download enclosures, transcripts, or chapters.",
        hints: HINT_NETWORK,
        words: &["podcast", "refresh"],
        slots: REFRESH,
        timeout: SHORT,
    },
    Tool {
        name: "podcast_text_show",
        description: "Show one fetched publisher transcript or chapter document. Cue times are publisher times, not media time, and the text is not an ASR row.",
        hints: HINT_QUERY,
        words: &["podcast", "text-show"],
        slots: &[pos("id", "Publisher text request id.")],
        timeout: SHORT,
    },
    Tool {
        name: "podcast_text",
        description: "Fetch one stored transcript or chapter document. Refresh does not do this. The recording quota does not change. Replay of the same id does not fetch again.",
        hints: HINT_NETWORK,
        words: &["podcast", "text"],
        slots: TEXT,
        timeout: SHORT,
    },
    Tool {
        name: "podcast_download",
        description: "Download one stored enclosure through the recording path. Reserves 512 MiB and 30 minutes before connect. Replay of the same recording id does not download again.",
        hints: HINT_NETWORK,
        words: &["podcast", "download"],
        slots: DOWNLOAD,
        timeout: Duration::from_secs(60),
    },
    Tool {
        name: "listen_file",
        description: "Play one protected retained interval through the service for at most 120 seconds. An explicit request replays its receipt without audio. destination null discards samples. This does not contact the source or stop capture.",
        hints: HINT_CHANGE,
        words: &["listen", "file"],
        slots: LISTEN_FILE,
        timeout: PLAY,
    },
    Tool {
        name: "listen_reader_show",
        description: "Inspect one durable retained-reader receipt without opening audio or releasing protection.",
        hints: HINT_QUERY,
        words: &["listen", "reader", "show"],
        slots: &[pos("id", "Retained-reader request id.")],
        timeout: SHORT,
    },
    Tool {
        name: "listen_reader_list",
        description: "List at most 16 retained-reader receipts, with unresolved protection first. Starts no work.",
        hints: HINT_QUERY,
        words: &["listen", "reader", "list"],
        slots: &[],
        timeout: SHORT,
    },
    Tool {
        name: "listen_attach",
        description: "Open one playhead on a capture, parked at the newest published segment. Does not start or stop capture.",
        hints: HINT_CHANGE,
        words: &["listen", "attach"],
        slots: ATTACH,
        timeout: SHORT,
    },
    Tool {
        name: "listen_pause",
        description: "Pause one playhead. Does not signal the capture worker and does not write a gap.",
        hints: HINT_CHANGE,
        words: &["listen", "pause"],
        slots: &[pos("session", "Playback session id.")],
        timeout: SHORT,
    },
    Tool {
        name: "listen_seek",
        description: "Move one playhead inside a published segment. A gap or the open tail is refused. Does not signal capture.",
        hints: HINT_CHANGE,
        words: &["listen", "seek"],
        slots: SEEK,
        timeout: SHORT,
    },
    Tool {
        name: "listen_live",
        description: "Park one playhead at the end of the newest published segment. Does not read the open tail.",
        hints: HINT_CHANGE,
        words: &["listen", "live"],
        slots: &[pos("session", "Playback session id.")],
        timeout: SHORT,
    },
    Tool {
        name: "listen_play",
        description: "Play one playhead's published segment for at most 120 seconds, then drop that playhead. Does not stop capture.",
        hints: HINT_CHANGE,
        words: &["listen", "play"],
        slots: PLAY_SESSION,
        timeout: PLAY,
    },
    Tool {
        name: "listen_session",
        description: "Show one playhead. No source URL or media path is included.",
        hints: HINT_QUERY,
        words: &["listen", "session"],
        slots: &[pos("session", "Playback session id.")],
        timeout: SHORT,
    },
    Tool {
        name: "listen_detach",
        description: "Drop one playhead. Does not stop the capture.",
        hints: HINT_CHANGE,
        words: &["listen", "detach"],
        slots: &[pos("session", "Playback session id.")],
        timeout: SHORT,
    },
    Tool {
        name: "listen_stop",
        description: "End one direct listen. Does not stop a recording.",
        hints: HINT_CHANGE,
        words: &["listen", "stop"],
        slots: &[pos("id", "Listen id.")],
        timeout: SHORT,
    },
    Tool {
        name: "radio_favorite",
        description: "Save one cached station as a favorite. Does not tune or record.",
        hints: HINT_CHANGE,
        words: &["radio", "favorite"],
        slots: &[pos("id", "Cached station id.")],
        timeout: SHORT,
    },
    Tool {
        name: "radio_unfavorite",
        description: "Remove one favorite. Does not delete the station, source, or recordings.",
        hints: HINT_CHANGE,
        words: &["radio", "unfavorite"],
        slots: &[pos("id", "Cached station id.")],
        timeout: SHORT,
    },
    Tool {
        name: "record_start",
        description: "Start one finite recording of an already registered source revision. Does not accept a URL. Radio attempts stay within 15 minutes and 256 MiB.",
        hints: HINT_NETWORK,
        words: &["record", "start"],
        slots: RECORD_START,
        timeout: SHORT,
    },
    Tool {
        name: "record_stop",
        description: "Finish the received portion of one running recording and validate it.",
        hints: HINT_CHANGE,
        words: &["record", "stop"],
        slots: &[pos("id", "Recording id.")],
        timeout: SHORT,
    },
    Tool {
        name: "record_pause",
        description: "Stop one running capture and record the uncovered plan as a gap. Does not write a silence file.",
        hints: HINT_CHANGE,
        words: &["record", "pause"],
        slots: &[pos("id", "Recording id.")],
        timeout: SHORT,
    },
    Tool {
        name: "record_keep",
        description: "Protect one recording from automatic expiration. This is not a backup.",
        hints: HINT_CHANGE,
        words: &["record", "keep"],
        slots: &[pos("id", "Recording id.")],
        timeout: SHORT,
    },
    Tool {
        name: "analysis_admit",
        description: "Pin one completed recording for analysis by its retained checksum and media clock. Does not transcribe or read a source URL.",
        hints: HINT_CHANGE,
        words: &["analysis", "admit"],
        slots: &[
            pos("id", "New analysis pin id."),
            text("recording", "--recording", true, "Completed recording id."),
        ],
        timeout: SHORT,
    },
    Tool {
        name: "analysis_publish",
        description: "Publish one pin revision so it can be transcribed. An older revision cannot replace a newer one.",
        hints: HINT_CHANGE,
        words: &["analysis", "publish"],
        slots: &[
            pos("id", "Analysis pin id."),
            required_count("revision", "--revision", 1, 64, "Pin revision."),
        ],
        timeout: SHORT,
    },
    Tool {
        name: "analysis_show",
        description: "Show one analysis pin: intervals, gaps and checksum.",
        hints: HINT_QUERY,
        words: &["analysis", "show"],
        slots: &[pos("id", "Analysis pin id.")],
        timeout: SHORT,
    },
    Tool {
        name: "analysis_transcribe",
        description: "Queue local speech recognition of a published pin with an already configured local profile. Runs on this machine, makes no network or paid request, and never reruns for the same job id. Profiles are configured only from the command line.",
        hints: HINT_CHANGE,
        words: &["analysis", "transcribe"],
        slots: &[
            pos("id", "New recognition job id."),
            text("input", "--input", true, "Published analysis pin id."),
            required_count("revision", "--revision", 1, 64, "Pin revision."),
            text(
                "profile",
                "--profile",
                true,
                "Configured recognition profile id.",
            ),
        ],
        timeout: SHORT,
    },
    Tool {
        name: "analysis_translate",
        description: "Queue local English translation of a recognized transcript with an already configured local profile. Runs on this machine, makes no network or paid request, and never reruns for the same job id.",
        hints: HINT_CHANGE,
        words: &["analysis", "translate"],
        slots: &[
            pos("id", "New translation job id."),
            text(
                "input",
                "--input",
                true,
                "Analysis pin id whose transcript is translated.",
            ),
            count(
                "transcript_revision",
                "--transcript-revision",
                1,
                64,
                "Transcript revision. Defaults to the newest.",
            ),
            text(
                "profile",
                "--profile",
                true,
                "Configured translation profile id.",
            ),
        ],
        timeout: SHORT,
    },
    Tool {
        name: "analysis_job",
        description: "Show one verification, recognition or translation job, including failures and cancellation.",
        hints: HINT_QUERY,
        words: &["analysis", "job"],
        slots: &[pos("id", "Job id.")],
        timeout: SHORT,
    },
    Tool {
        name: "analysis_cancel",
        description: "Cancel one exact job generation. Its inputs stay protected until the work actually stops.",
        hints: HINT_STOP,
        words: &["analysis", "cancel"],
        slots: &[
            pos("id", "Job id."),
            required_count("generation", "--generation", 1, 64, "Job generation."),
        ],
        timeout: SHORT,
    },
    Tool {
        name: "analysis_correct",
        description: "Append one transcript revision that replaces one cue's original script and copies the other cues. Name the revision last read. A second edit of that same revision conflicts. The previous revision stays readable. This does not start recognition, translation, or a paid request, and it does not restore a deleted recording.",
        hints: HINT_CHANGE,
        words: &["analysis", "correct"],
        slots: &[
            pos("id", "Analysis pin id."),
            required_count(
                "expect",
                "--expect",
                1,
                63,
                "Transcript revision last read. A newer revision conflicts.",
            ),
            required_count("ordinal", "--ordinal", 0, 255, "Cue ordinal to replace."),
            text(
                "text",
                "--text",
                true,
                "Replacement original-script text for that cue.",
            ),
        ],
        timeout: SHORT,
    },
    Tool {
        name: "analysis_transcript",
        description: "Read recognized cues in the original script with media times. Machine output, unreviewed; quote it as such.",
        hints: HINT_QUERY,
        words: &["analysis", "transcript"],
        slots: &[
            pos("id", "Analysis pin id."),
            count(
                "revision",
                "--revision",
                1,
                64,
                "Transcript revision. Defaults to the newest.",
            ),
            count(
                "after",
                "--after",
                0,
                255,
                "Continue after this cue ordinal.",
            ),
        ],
        timeout: SHORT,
    },
    Tool {
        name: "analysis_translation",
        description: "Read original and English cue pairs with media times and reasons for untranslated cues. Machine translation, unreviewed; the original is the evidence.",
        hints: HINT_QUERY,
        words: &["analysis", "translation"],
        slots: &[
            pos("id", "Analysis pin id."),
            count(
                "transcript_revision",
                "--transcript-revision",
                1,
                64,
                "Transcript revision. Defaults to the newest.",
            ),
            count(
                "revision",
                "--revision",
                1,
                64,
                "Translation revision. Defaults to the newest.",
            ),
            count(
                "after",
                "--after",
                0,
                255,
                "Continue after this cue ordinal.",
            ),
        ],
        timeout: SHORT,
    },
    Tool {
        name: "analysis_search",
        description: "Find a literal term in stored original-script cues and English translations across this library, matching as written and ignoring letter case only, like monitor_matches. Each hit cites source, recording, transcript and translation revisions, cue and media time, says whether those revisions are current or stale, and whether the audio is retained, released, expired or missing. Reads only: no job, finding, briefing or network request. Bounded by limit, scan_rows and deadline_ms; a stopped page returns a cursor for after. Machine output, unreviewed; a hit is a place to check, not a finding.",
        hints: HINT_QUERY,
        words: &["analysis", "search"],
        slots: SEARCH,
        timeout: SHORT,
    },
    Tool {
        name: "monitor_list",
        description: "List monitor ids. Monitors are created and revised only by the user through the CLI.",
        hints: HINT_QUERY,
        words: &["monitor", "list"],
        slots: &[],
        timeout: SHORT,
    },
    Tool {
        name: "monitor_show",
        description: "Show a monitor's current user version (goal, literal terms, sources, approved candidates, audio caps), whether it is paused, and the sources it follows now.",
        hints: HINT_QUERY,
        words: &["monitor", "show"],
        slots: &[
            pos("id", "Monitor id."),
            count("version", "--version", 1, 1000, "A historical version."),
        ],
        timeout: SHORT,
    },
    Tool {
        name: "monitor_actions",
        description: "List a monitor's recorded proposals with origin, the version each was checked against, and the applied or refused decision with its reason.",
        hints: HINT_QUERY,
        words: &["monitor", "actions"],
        slots: &[
            pos("id", "Monitor id."),
            count(
                "after",
                "--after",
                0,
                100_000,
                "Continue after this action ordinal.",
            ),
        ],
        timeout: SHORT,
    },
    Tool {
        name: "monitor_coverage",
        description: "Count each stage for the sources a monitor follows: captures, published audio, gaps, pins, transcripts with and without text, translated and untranslated cues, and missed schedule windows. Report coverage before any conclusion; a stage count is not a percentage of the world.",
        hints: HINT_QUERY,
        words: &["monitor", "coverage"],
        slots: WINDOW,
        timeout: SHORT,
    },
    Tool {
        name: "monitor_matches",
        description: "Find a monitor's literal terms in the latest transcripts and English translations, citing recording, transcript revision, cue and media time. Matching ignores letter case only. A match is a place to check, not a finding; recognized text and translations are unreviewed machine output.",
        hints: HINT_QUERY,
        words: &["monitor", "matches"],
        slots: WINDOW,
        timeout: SHORT,
    },
    Tool {
        name: "monitor_propose",
        description: "Record a model proposal for a monitor. The service applies it only if the user's current version already allows it, such as adding an approved candidate source; anything else, including raising a cap or enabling paid processing, is refused and kept with its reason. The origin is always model. An action id is an idempotency key.",
        hints: HINT_CHANGE,
        words: &["monitor", "propose", "--origin", "model"],
        slots: &[
            pos("id", "Monitor id."),
            text(
                "action_id",
                "--action-id",
                true,
                "Idempotency key for this proposal.",
            ),
            text(
                "add_source",
                "--add-source",
                false,
                "Source revision to add. Applied only if the version lists it or approved it as a candidate.",
            ),
            text(
                "remove_source",
                "--remove-source",
                false,
                "Source revision to stop following. The last source cannot be removed.",
            ),
            text(
                "request",
                "--request",
                false,
                "Any other request, kept verbatim and refused.",
            ),
        ],
        timeout: SHORT,
    },
];

const WINDOW: &[Slot] = &[
    pos("id", "Monitor id."),
    count(
        "hours",
        "--hours",
        1,
        744,
        "Hours of capture start times before now. Defaults to 24.",
    ),
    count(
        "from_ms",
        "--from-ms",
        0,
        4_102_444_800_000,
        "Exact window start in Unix milliseconds; requires to_ms.",
    ),
    count(
        "to_ms",
        "--to-ms",
        0,
        4_102_444_800_000,
        "Exact window end (exclusive) in Unix milliseconds; requires from_ms.",
    ),
];

const SEARCH: &[Slot] = &[
    text(
        "term",
        "--term",
        true,
        "Literal term, at most 200 characters, in any script.",
    ),
    text(
        "in",
        "--in",
        false,
        "original, english or both. Defaults to both.",
    ),
    flag(
        "history",
        "--history",
        "Also search older transcript and translation revisions; each hit says whether it is stale.",
    ),
    text("source", "--source", false, "Only this source revision."),
    count(
        "from_ms",
        "--from-ms",
        0,
        4_102_444_800_000,
        "Earliest capture start in Unix milliseconds; requires to_ms.",
    ),
    count(
        "to_ms",
        "--to-ms",
        0,
        4_102_444_800_000,
        "Capture start before this time in Unix milliseconds; requires from_ms.",
    ),
    text(
        "language",
        "--language",
        false,
        "Only cues a stored language label overlaps, such as fr or fr-CA. Labels are unevaluated evidence.",
    ),
    count("limit", "--limit", 1, 64, "Most hits. Defaults to 16."),
    count(
        "scan_rows",
        "--scan-rows",
        2,
        200_000,
        "Most catalog rows read. Defaults to 20000.",
    ),
    count(
        "deadline_ms",
        "--deadline-ms",
        10,
        2_000,
        "Wall-time bound in milliseconds. Defaults to 1000.",
    ),
    text(
        "after",
        "--after",
        false,
        "Cursor from the previous page, with the same term and options.",
    ),
];

const fn pos(key: &'static str, description: &'static str) -> Slot {
    Slot {
        key,
        kind: Field::Text,
        flag: "",
        required: true,
        minimum: 0,
        maximum: MAX_TEXT,
        description,
    }
}

const fn text(
    key: &'static str,
    flag: &'static str,
    required: bool,
    description: &'static str,
) -> Slot {
    text_bound(key, flag, required, MAX_TEXT, description)
}

const fn text_bound(
    key: &'static str,
    flag: &'static str,
    required: bool,
    maximum: u64,
    description: &'static str,
) -> Slot {
    Slot {
        key,
        kind: Field::Text,
        flag,
        required,
        minimum: 0,
        maximum,
        description,
    }
}

const fn count(
    key: &'static str,
    flag: &'static str,
    minimum: u64,
    maximum: u64,
    description: &'static str,
) -> Slot {
    Slot {
        key,
        kind: Field::Count,
        flag,
        required: false,
        minimum,
        maximum,
        description,
    }
}

const fn required_count(
    key: &'static str,
    flag: &'static str,
    minimum: u64,
    maximum: u64,
    description: &'static str,
) -> Slot {
    Slot {
        key,
        kind: Field::Count,
        flag,
        required: true,
        minimum,
        maximum,
        description,
    }
}

const fn flag(key: &'static str, cli: &'static str, description: &'static str) -> Slot {
    Slot {
        key,
        kind: Field::Flag,
        flag: cli,
        required: false,
        minimum: 0,
        maximum: 0,
        description,
    }
}

const PAGE: &[Slot] = &[
    text(
        "after",
        "--after",
        false,
        "Page cursor returned by the previous page.",
    ),
    count("limit", "--limit", 1, 32, "Page size."),
];

const RADIO_SEARCH: &[Slot] = &[
    text("name", "--name", false, "Station name substring."),
    text(
        "country",
        "--country",
        false,
        "Unique country name or raw two-letter code; ambiguous names require a code.",
    ),
    text(
        "language",
        "--language",
        false,
        "Directory language label, not detected speech.",
    ),
    text("tag", "--tag", false, "Directory tag."),
    flag(
        "healthy",
        "--healthy",
        "Keep only stations whose latest directory check succeeded.",
    ),
    flag("favorites", "--favorites", "Keep only saved favorites."),
    Slot {
        kind: Field::StationOrder,
        ..text(
            "order",
            "--order",
            false,
            "id (default) preserves UUID order; name uses the pinned Unicode name key with UUID ties.",
        )
    },
    text_bound(
        "after",
        "--after",
        false,
        8_192,
        "At most 8192 UTF-8 bytes. With id: UUID; with name: opaque cursor, same filters and limit. On changed cache omit after to restart.",
    ),
    count("limit", "--limit", 1, 16, "Page size, 1 to 16."),
];

const EPISODES: &[Slot] = &[
    pos("subscription", "Subscription id."),
    text("after", "--after", false, "Page cursor."),
    count("limit", "--limit", 1, 32, "Page size."),
];

const SUBSCRIBE: &[Slot] = &[
    pos("id", "New subscription id."),
    text(
        "url",
        "--url",
        true,
        "Feed URL. Stored as given after normalization. Not fetched by this tool.",
    ),
    text(
        "pin_address",
        "--pin-address",
        false,
        "Optional exact IP pin.",
    ),
    text(
        "redirects",
        "--redirects",
        false,
        "deny, same-origin, or public.",
    ),
];

const REFRESH: &[Slot] = &[
    pos("subscription", "Subscription id."),
    text(
        "id",
        "--id",
        true,
        "New refresh id. Reuse does not fetch again.",
    ),
];

const TEXT: &[Slot] = &[
    pos("subscription", "Subscription id."),
    text(
        "episode",
        "--episode",
        true,
        "Episode id from podcast_episodes.",
    ),
    text("kind", "--kind", true, "transcript or chapters."),
    required_count("index", "--index", 0, 7, "Zero-based asset index."),
    text(
        "id",
        "--id",
        true,
        "Request id. Reuse does not fetch again.",
    ),
];

const DOWNLOAD: &[Slot] = &[
    pos("subscription", "Subscription id."),
    text(
        "episode",
        "--episode",
        true,
        "Episode id from podcast_episodes.",
    ),
    text("id", "--id", true, "Recording id."),
    text(
        "revision",
        "--revision",
        true,
        "New source revision id for the enclosure.",
    ),
];

const ATTACH: &[Slot] = &[
    pos("session", "Playback session id."),
    text(
        "recording",
        "--recording",
        true,
        "Recording id. Not a source URL.",
    ),
];

const SEEK: &[Slot] = &[
    pos("session", "Playback session id."),
    required_count(
        "seek_us",
        "--seek-us",
        0,
        3_600_000_000,
        "Timeline offset in microseconds, inside one published segment.",
    ),
];

const PLAY_SESSION: &[Slot] = &[
    pos("session", "Playback session id."),
    text(
        "destination",
        "--destination",
        true,
        "null discards samples. system uses a local device when the decoder has one.",
    ),
];

const LISTEN_FILE: &[Slot] = &[
    pos("id", "Retained recording id."),
    text(
        "destination",
        "--destination",
        true,
        "null discards samples. system uses a local device when the decoder has one.",
    ),
    count(
        "seek_us",
        "--seek-us",
        0,
        3_600_000_000,
        "Start offset in microseconds, inside the published duration.",
    ),
    text_bound(
        "request",
        "--request",
        false,
        128,
        "Exact replay identity. ASCII letters, digits, hyphen, underscore, colon or period; at most 128 bytes.",
    ),
];

const RECORD_START: &[Slot] = &[
    pos("id", "Recording id. Reuse reconciles the same attempt."),
    text(
        "source",
        "--source",
        true,
        "Registered source revision. Not a URL.",
    ),
    count(
        "seconds",
        "--seconds",
        1,
        900,
        "Finite duration in seconds.",
    ),
    count("max_mib", "--max-mib", 1, 256, "Byte ceiling in MiB."),
    text(
        "retention",
        "--retention",
        false,
        "temporary, kept, or archived.",
    ),
    flag(
        "icy",
        "--icy",
        "Request ICY titles. They are observations and are not written into the audio.",
    ),
];

#[cfg(test)]
mod tests {
    #[test]
    fn retained_reader_adapter_preserves_request_and_query_authority() -> Result<(), String> {
        use super::*;
        let file = TOOLS
            .iter()
            .find(|tool| tool.name == "listen_file")
            .ok_or("file tool")?;
        let value = json!({"id":"recording", "destination":"null", "request":"reader:one"});
        assert_eq!(
            argv(file, value.as_object().ok_or("object")?)?,
            [
                "listen",
                "file",
                "recording",
                "--destination",
                "null",
                "--request",
                "reader:one"
            ]
        );
        let oversized = json!({"id":"recording", "destination":"null", "request":"x".repeat(129)});
        assert!(argv(file, oversized.as_object().ok_or("object")?).is_err());
        for name in ["listen_reader_show", "listen_reader_list"] {
            let tool = TOOLS
                .iter()
                .find(|tool| tool.name == name)
                .ok_or("reader tool")?;
            assert_eq!(tool.hints, HINT_QUERY);
            let escaped = json!({"id":"reader", "data_dir":"other", "shell":"execute"});
            assert!(argv(tool, escaped.as_object().ok_or("object")?).is_err());
        }
        assert!(TOOLS.iter().all(|tool| tool.name != "listen_reader_stop"));
        Ok(())
    }
    use super::*;

    #[test]
    fn cached_radio_search_order_maps_exact_args_and_preserves_id_default() -> Result<(), String> {
        let tool = TOOLS
            .iter()
            .find(|tool| tool.name == "radio_search")
            .ok_or("radio_search")?;
        assert_eq!(argv(tool, &Map::new())?, ["radio", "search"]);
        for order in ["id", "name"] {
            let arguments = json!({"country":"Canada", "favorites":true, "order":order, "after":"af12", "limit":16});
            assert_eq!(
                argv(tool, arguments.as_object().ok_or("object")?)?,
                [
                    "radio",
                    "search",
                    "--country",
                    "Canada",
                    "--favorites",
                    "--order",
                    order,
                    "--after",
                    "af12",
                    "--limit",
                    "16"
                ]
            );
        }
        for arguments in [
            json!({"order":"popularity"}),
            json!({"order":"NAME"}),
            json!({"limit":17}),
            json!({"limit":0}),
        ] {
            assert!(argv(tool, arguments.as_object().ok_or("object")?).is_err());
        }
        let schema = tool_schema(tool);
        assert_eq!(
            schema["inputSchema"]["properties"]["order"]["enum"],
            json!(["id", "name"])
        );
        assert_eq!(schema["inputSchema"]["properties"]["limit"]["minimum"], 1);
        assert_eq!(schema["inputSchema"]["properties"]["limit"]["maximum"], 16);
        assert_eq!(schema["annotations"]["readOnlyHint"], true);
        assert_eq!(schema["annotations"]["openWorldHint"], false);
        Ok(())
    }

    #[test]
    fn only_radio_cursor_accepts_8192_bytes_and_unrelated_text_stays_bounded() -> Result<(), String>
    {
        let tool = TOOLS
            .iter()
            .find(|tool| tool.name == "radio_search")
            .ok_or("radio_search")?;
        let cursor = "af".repeat(4096);
        let arguments = json!({"order":"name", "after":cursor});
        assert_eq!(
            argv(tool, arguments.as_object().ok_or("object")?)?,
            [
                "radio",
                "search",
                "--order",
                "name",
                "--after",
                cursor.as_str()
            ]
        );
        for arguments in [
            json!({"after":format!("{cursor}a")}),
            json!({"name":"a".repeat(2049)}),
            json!({"after":"af\n12"}),
        ] {
            assert!(argv(tool, arguments.as_object().ok_or("object")?).is_err());
        }
        assert!(
            argv(
                tool,
                json!({"name":"a".repeat(2048)})
                    .as_object()
                    .ok_or("object")?
            )
            .is_ok()
        );
        let show = TOOLS
            .iter()
            .find(|tool| tool.name == "radio_show")
            .ok_or("radio_show")?;
        assert!(
            argv(
                show,
                json!({"id":"a".repeat(2049)}).as_object().ok_or("object")?
            )
            .is_err()
        );
        let schema = tool_schema(tool);
        assert_eq!(
            schema["inputSchema"]["properties"]["after"]["maxLength"],
            8192
        );
        assert_eq!(
            schema["inputSchema"]["properties"]["after"]["x-sigy-maxBytes"],
            8192
        );
        assert_eq!(
            schema["inputSchema"]["properties"]["name"]["maxLength"],
            2048
        );
        Ok(())
    }

    #[test]
    fn archive_search_is_read_only_and_keeps_the_term_an_option_value() -> Result<(), String> {
        let tool = TOOLS
            .iter()
            .find(|tool| tool.name == "analysis_search")
            .ok_or("analysis_search")?;
        let schema = tool_schema(tool);
        if schema["annotations"]["readOnlyHint"] != true
            || schema["annotations"]["openWorldHint"] != false
            || schema["inputSchema"]["required"] != json!(["term"])
        {
            return Err(schema.to_string());
        }
        let arguments = json!({"term": "--data-dir=elsewhere", "history": true, "limit": 3});
        let args = argv(tool, arguments.as_object().ok_or("object")?)?;
        if args
            != [
                "analysis",
                "search",
                "--term",
                "--data-dir=elsewhere",
                "--history",
                "--limit",
                "3",
            ]
        {
            return Err(format!("{args:?}"));
        }
        for refused in [
            json!({}),
            json!({"term": "x", "data_dir": "elsewhere"}),
            json!({"term": "x", "limit": 65}),
            json!({"term": "x", "scan_rows": 1}),
            json!({"term": "x", "deadline_ms": 2_001}),
            json!({"term": "line
break"}),
        ] {
            if argv(tool, refused.as_object().ok_or("object")?).is_ok() {
                return Err(refused.to_string());
            }
        }
        Ok(())
    }

    #[test]
    fn positional_arguments_cannot_start_with_hyphen() -> Result<(), String> {
        let tool = TOOLS
            .iter()
            .find(|tool| tool.name == "radio_show")
            .ok_or("radio_show")?;
        for flag in ["-h", "--help", "--data-dir", "-v", "--version"] {
            let arguments = json!({"id": flag});
            assert!(
                argv(tool, arguments.as_object().ok_or("object")?).is_err(),
                "positional {flag} should have been rejected"
            );
        }
        let valid = json!({"id": "station-123"});
        assert_eq!(
            argv(tool, valid.as_object().ok_or("object")?)?,
            ["radio", "show", "station-123"]
        );
        Ok(())
    }
}
