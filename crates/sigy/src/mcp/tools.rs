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
const MAX_TEXT: usize = 2_048;
const MAX_OUTPUT: usize = 65_536;

#[derive(Clone, Copy)]
enum Field {
    Text,
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
                    Field::Text => "string",
                    Field::Count => "integer",
                    Field::Flag => "boolean",
                },
                "description": slot.description,
            }),
        );
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
        Field::Text => {
            let text = value
                .as_str()
                .filter(|text| bounded(text))
                .ok_or_else(|| format!("{} must be a short string", slot.key))?;
            if slot.flag.is_empty() {
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

fn bounded(text: &str) -> bool {
    !text.is_empty() && text.len() <= MAX_TEXT && !text.chars().any(char::is_control)
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
            return Err(format!(
                "tool exceeded {} seconds; if the service already admitted the job, it keeps running",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(pipe) = child.stdout.take() {
        let _ = pipe
            .take(u64::try_from(MAX_OUTPUT).unwrap_or(u64::MAX))
            .read_to_end(&mut stdout);
    }
    if let Some(pipe) = child.stderr.take() {
        let _ = pipe
            .take(u64::try_from(MAX_OUTPUT).unwrap_or(u64::MAX))
            .read_to_end(&mut stderr);
    }
    let stdout = String::from_utf8_lossy(&stdout);
    let stderr = String::from_utf8_lossy(&stderr);
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
        description: "Show one direct-listen receipt. Episode playback does not create a receipt.",
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
        description: "Play one retained recording in this process for at most 120 seconds. destination null discards samples. This does not contact the source or change retention.",
        hints: HINT_CHANGE,
        words: &["listen", "file"],
        slots: LISTEN_FILE,
        timeout: PLAY,
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
        name: "record_keep",
        description: "Protect one recording from automatic expiration. This is not a backup.",
        hints: HINT_CHANGE,
        words: &["record", "keep"],
        slots: &[pos("id", "Recording id.")],
        timeout: SHORT,
    },
];

const fn pos(key: &'static str, description: &'static str) -> Slot {
    Slot {
        key,
        kind: Field::Text,
        flag: "",
        required: true,
        minimum: 0,
        maximum: 0,
        description,
    }
}

const fn text(
    key: &'static str,
    flag: &'static str,
    required: bool,
    description: &'static str,
) -> Slot {
    Slot {
        key,
        kind: Field::Text,
        flag,
        required,
        minimum: 0,
        maximum: 0,
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
    text("country", "--country", false, "Two-letter country code."),
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
    text("after", "--after", false, "Page cursor."),
    count("limit", "--limit", 1, 32, "Page size."),
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
