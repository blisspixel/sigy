use std::{
    collections::HashMap,
    fmt::Write as _,
    sync::{Arc, Mutex},
    time::Duration,
};

use reqwest::Url;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use super::{
    live::{self, LiveGap},
    master_candidates,
    media::{self, MAX_FINITE_SEGMENTS, MAX_LIVE_WINDOW, Mode},
};
use crate::{
    Error,
    sources::{
        HttpSource, NetworkScope, RedirectPolicy,
        http::{AcquisitionLimits, AudioContentType, HttpAcquirer, TransferEnd},
        playlist::{CandidateKind, authorize_entry, playlist_lines},
    },
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn parent(path: &str) -> Result<HttpSource, Error> {
    HttpSource::new(
        "Playlist",
        &format!("http://127.0.0.1:9/{path}"),
        NetworkScope::PinnedAddress {
            address: std::net::Ipv4Addr::LOCALHOST.into(),
        },
    )?
    .with_redirects(RedirectPolicy::Deny)
}

fn url(path: &str) -> Result<Url, Error> {
    Url::parse(&format!("http://127.0.0.1:9/{path}")).map_err(|_| Error::InvalidInput("url"))
}

fn invalid(result: &Result<media::MediaPlaylist, Error>, message: &str) -> bool {
    matches!(result, Err(Error::InvalidInput(text)) if *text == message)
}

#[test]
fn master_playlist_fails_before_any_segment_is_authorized() {
    let error = media::parse(
        b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nhttp://secret.example/variant.m3u8\n",
        Mode::Finite,
    );
    assert!(invalid(
        &error,
        "HLS master playlist; resolve it and accept one variant"
    ));
    let live = media::parse(
        b"#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"a\",URI=\"x\"\n",
        Mode::Live,
    );
    assert!(invalid(
        &live,
        "HLS master playlist; resolve it and accept one variant"
    ));
}

#[test]
fn finite_playlist_keeps_bounded_same_origin_segments() -> TestResult {
    let source = parent("media.m3u8")?;
    let base = url("media.m3u8")?;
    let playlist = media::parse(
        b"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\nseg0\n#EXTINF:1,\nseg1\n#EXT-X-ENDLIST\n",
        Mode::Finite,
    )?;
    assert!(playlist.ended);
    assert_eq!(playlist.segments.len(), 2);
    let first = authorize_entry(&source, &base, &playlist.segments[0].reference)?;
    assert_eq!(first.endpoint(), "http://127.0.0.1:9/seg0");
    let allowed = media::parse(
        b"#EXTM3U\n## a comment\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:4\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-PROGRAM-DATE-TIME:2026-09-24T00:00:00Z\n#EXTINF:1,title, with comma\nseg0\n#EXT-X-ENDLIST\n",
        Mode::Finite,
    )?;
    assert_eq!(allowed.segments[0].sequence, 4);
    let foreign = media::parse(
        b"#EXTM3U\n#EXTINF:1,\nhttp://secret.example/seg0\n#EXT-X-ENDLIST\n",
        Mode::Finite,
    )?;
    assert!(matches!(
        authorize_entry(&source, &base, &foreign.segments[0].reference),
        Err(Error::DestinationDenied)
    ));
    let mut bounded = String::from("#EXTM3U\n");
    for index in 0..=MAX_FINITE_SEGMENTS {
        writeln!(bounded, "#EXTINF:1,\nseg{index}")?;
    }
    bounded.push_str("#EXT-X-ENDLIST\n");
    assert!(invalid(
        &media::parse(bounded.as_bytes(), Mode::Finite),
        "playlist entry limit"
    ));
    Ok(())
}

/// Hostile or unsupported media playlists and the exact refusal each one gets.
const HOSTILE_MEDIA: &[(&[u8], Mode, &str)] = &[
        (b"seg0\n", Mode::Finite, "HLS playlist is not accepted"),
        (b"#EXTINF:1,\nseg0\n#EXTM3U\n", Mode::Finite, "HLS playlist is not accepted"),
        (
            b"#EXTM3U\n#EXTINF:1,\nhttp://secret.example/live\n",
            Mode::Finite,
            "HLS live playlist requires record hls --live",
        ),
        (
            b"#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key\"\n#EXTINF:1,\nseg0\n#EXT-X-ENDLIST\n",
            Mode::Finite,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:1,\nseg0\n",
            Mode::Live,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXT-X-MAP:URI=\"init\"\n#EXTINF:1,\nseg0\n#EXT-X-ENDLIST\n",
            Mode::Finite,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXTINF:1,\n#EXT-X-BYTERANGE:1\nseg0\n#EXT-X-ENDLIST\n",
            Mode::Finite,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXT-X-DISCONTINUITY\n#EXTINF:1,\nseg0\n#EXT-X-ENDLIST\n",
            Mode::Finite,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-PART:DURATION=0.1,URI=\"part\"\n#EXTINF:1,\nseg0\n",
            Mode::Live,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-DEFINE:NAME=\"a\",VALUE=\"b\"\n#EXTINF:1,\n{$a}\n",
            Mode::Live,
            "unsupported HLS playlist",
        ),
        (b"#EXTM3U\n#EXT-X-ENDLIST\n", Mode::Finite, "playlist has no entries"),
        (
            b"#EXTM3U\n#EXTINF\xC3\xA9\nseg0\n#EXT-X-ENDLIST\n",
            Mode::Finite,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\nseg0\n#EXT-X-ENDLIST\n",
            Mode::Finite,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXTINF:1,\n#EXTINF:1,\nseg0\n#EXT-X-ENDLIST\n",
            Mode::Finite,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXTINF:1,\nseg0\n#EXTINF:1,\n#EXT-X-ENDLIST\n",
            Mode::Finite,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXTINF:-1,\nseg0\n#EXT-X-ENDLIST\n",
            Mode::Finite,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXTINF:0,\nseg0\n#EXT-X-ENDLIST\n",
            Mode::Finite,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXT-X-UNKNOWN-TAG:1\n#EXTINF:1,\nseg0\n#EXT-X-ENDLIST\n",
            Mode::Finite,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXTINF:1,\nseg0\n#EXT-X-MEDIA-SEQUENCE:3\n#EXT-X-ENDLIST\n",
            Mode::Finite,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:1\n#EXT-X-MEDIA-SEQUENCE:2\n#EXTINF:1,\nseg0\n#EXT-X-ENDLIST\n",
            Mode::Finite,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXT-X-TARGETDURATION:0\n#EXTINF:1,\nseg0\n#EXT-X-ENDLIST\n",
            Mode::Finite,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:18446744073709551615\n#EXTINF:1,\nseg0\n#EXTINF:1,\nseg1\n#EXT-X-ENDLIST\n",
            Mode::Finite,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXTINF:1,\nseg0\n",
            Mode::Live,
            "HLS live playlist needs a target duration of 1 to 60 seconds",
        ),
        (
            b"#EXTM3U\n#EXT-X-TARGETDURATION:61\n#EXTINF:1,\nseg0\n",
            Mode::Live,
            "HLS live playlist needs a target duration of 1 to 60 seconds",
        ),
        (
            b"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-DISCONTINUITY\n#EXT-X-DISCONTINUITY\n#EXTINF:1,\nseg0\n",
            Mode::Live,
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\nseg0\n#EXT-X-DISCONTINUITY\n",
            Mode::Live,
            "unsupported HLS playlist",
        ),
        (b"#EXTM3U\n#EXTINF:1,\nseg\x01\n", Mode::Finite, "invalid playlist document"),
    ];

#[test]
fn unsupported_and_hostile_media_playlists_fail_closed() {
    for (body, mode, message) in HOSTILE_MEDIA {
        let result = media::parse(body, *mode);
        assert!(invalid(&result, message), "{body:?} -> {result:?}");
    }
}

#[test]
fn live_window_keeps_sequence_numbers_and_discontinuity_epochs() -> TestResult {
    let playlist = media::parse(
        b"#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:100\n#EXT-X-DISCONTINUITY-SEQUENCE:5\n#EXT-X-PROGRAM-DATE-TIME:2026-09-24T00:00:00Z\n#EXTINF:6.006,\na.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:6.006,\nb.ts\n#EXTINF:5.5,\n#EXT-X-DISCONTINUITY\nc.ts\n#EXTINF:6,\nd.ts\n",
        Mode::Live,
    )?;
    assert!(!playlist.ended);
    assert_eq!(playlist.target_seconds, Some(6));
    let observed: Vec<(u64, u64, &str)> = playlist
        .segments
        .iter()
        .map(|segment| (segment.sequence, segment.epoch, segment.reference.as_str()))
        .collect();
    assert_eq!(
        observed,
        [
            (100, 5, "a.ts"),
            (101, 6, "b.ts"),
            (102, 7, "c.ts"),
            (103, 7, "d.ts")
        ]
    );
    let empty = media::parse(
        b"#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:9\n",
        Mode::Live,
    )?;
    assert!(empty.segments.is_empty());
    assert_eq!(empty.media_sequence, 9);
    let mut window = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:1\n");
    for index in 0..=MAX_LIVE_WINDOW {
        writeln!(window, "#EXTINF:1,\n{index}")?;
    }
    assert!(invalid(
        &media::parse(window.as_bytes(), Mode::Live),
        "playlist entry limit"
    ));
    Ok(())
}

fn master(body: &[u8]) -> Result<Vec<crate::sources::playlist::ResolvedEntry>, Error> {
    let lines = playlist_lines(body)?;
    master_candidates(
        &parent("live/master.m3u8")?,
        &url("live/master.m3u8")?,
        &lines,
    )
}

#[test]
fn master_playlist_lists_variants_without_choosing_or_fetching() -> TestResult {
    let entries = master(
        b"#EXTM3U\n#EXT-X-VERSION:4\n#EXT-X-INDEPENDENT-SEGMENTS\n## comment\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"Arabic\",LANGUAGE=\"ar\",URI=\"audio/ar.m3u8\"\n#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"sub\",NAME=\"en\",URI=\"subs.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=2500000,CODECS=\"avc1.4d401f,mp4a.40.2\",RESOLUTION=1280x720,AUDIO=\"aud\"\nhd.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=64000,CODECS=\"mp4a.40.2\"\n/radio/low.m3u8?token=hidden\n#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=1000,URI=\"iframe.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=96000\nplain.m3u8\n",
    )?;
    let listed: Vec<_> = entries
        .iter()
        .map(|entry| {
            (
                entry.kind,
                entry.endpoint.as_str(),
                entry.bandwidth,
                entry.codecs.as_deref(),
                entry.audio_only,
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            (
                CandidateKind::HlsAudio,
                "http://127.0.0.1:9/live/audio/ar.m3u8",
                None,
                None,
                Some(true)
            ),
            (
                CandidateKind::HlsVariant,
                "http://127.0.0.1:9/live/hd.m3u8",
                Some(2_500_000),
                Some("avc1.4d401f,mp4a.40.2"),
                Some(false)
            ),
            (
                CandidateKind::HlsVariant,
                "http://127.0.0.1:9/radio/low.m3u8?token=hidden",
                Some(64_000),
                Some("mp4a.40.2"),
                Some(true)
            ),
            (
                CandidateKind::HlsVariant,
                "http://127.0.0.1:9/live/plain.m3u8",
                Some(96_000),
                None,
                None
            ),
        ]
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry.origin == "http://127.0.0.1:9")
    );
    Ok(())
}

#[test]
fn hostile_master_playlists_fail_closed() -> TestResult {
    let cases: &[(&[u8], &str)] = &[
        (
            b"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\nseg\n",
            "HLS media playlist is recorded with record hls",
        ),
        (b"#EXT-X-STREAM-INF:BANDWIDTH=1\nv\n", "HLS playlist is not accepted"),
        (b"#EXTM3U\n#EXT-X-STREAM-INF:CODECS=\"mp4a.40.2\"\nv\n", "unsupported HLS playlist"),
        (b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=0\nv\n", "unsupported HLS playlist"),
        (b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=-5\nv\n", "unsupported HLS playlist"),
        (
            b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=10000000001\nv\n",
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1,CODECS=\"mp4a;rm -rf\"\nv\n",
            "unsupported HLS playlist",
        ),
        (b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\n#EXT-X-VERSION:3\nv\n", "unsupported HLS playlist"),
        (b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\n", "unsupported HLS playlist"),
        (b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv\nstray\n", "unsupported HLS playlist"),
        (
            b"#EXTM3U\n#EXT-X-SESSION-KEY:METHOD=AES-128,URI=\"k\"\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv\n",
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXT-X-DEFINE:NAME=\"a\",VALUE=\"b\"\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv\n",
            "unsupported HLS playlist",
        ),
        (
            b"#EXTM3U\n#EXT-X-MEDIA:TYPE=VIDEO,GROUP-ID=\"v\",NAME=\"v\",URI=\"v.m3u8\"\n",
            "playlist has no entries",
        ),
        (b"#EXTM3U\n#EXT-X-MEDIA:GROUP-ID=\"a\",URI=\"a.m3u8\"\n", "unsupported HLS playlist"),
        (b"#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,URI=a.m3u8\n", "unsupported HLS playlist"),
    ];
    for (body, message) in cases {
        let result = master(body);
        assert!(
            matches!(&result, Err(Error::InvalidInput(text)) if text == message),
            "{body:?} -> {:?}",
            result.map(|entries| entries.len())
        );
    }
    assert!(matches!(
        master(b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nhttp://secret.example/v.m3u8\n"),
        Err(Error::DestinationDenied)
    ));
    let mut many = String::from("#EXTM3U\n");
    for index in 0..33 {
        writeln!(many, "#EXT-X-STREAM-INF:BANDWIDTH=1\nv{index}.m3u8")?;
    }
    assert!(matches!(
        master(many.as_bytes()),
        Err(Error::InvalidInput("playlist entry limit"))
    ));
    Ok(())
}

type Responder = Arc<dyn Fn(&str, usize) -> (u16, &'static str, Vec<u8>) + Send + Sync>;

struct Fixture {
    port: u16,
    paths: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Fixture {
    async fn start(responder: Responder) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let paths = Arc::new(Mutex::new(Vec::new()));
        let seen = paths.clone();
        let task = tokio::spawn(async move {
            let mut counts: HashMap<String, usize> = HashMap::new();
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut request = Vec::new();
                while request.len() < 4096 && !request.ends_with(b"\r\n\r\n") {
                    match socket.read_u8().await {
                        Ok(byte) => request.push(byte),
                        Err(_) => break,
                    }
                }
                let text = String::from_utf8_lossy(&request);
                let path = text.split_whitespace().nth(1).unwrap_or("").to_owned();
                let count = counts.entry(path.clone()).or_insert(0);
                let (status, kind, body) = responder(&path, *count);
                *count += 1;
                if let Ok(mut list) = seen.lock() {
                    list.push(path);
                }
                let head = format!(
                    "HTTP/1.1 {status} Fixture\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
                let _ = socket.shutdown().await;
            }
        });
        Ok(Self { port, paths, task })
    }

    fn source(&self) -> Result<HttpSource, Error> {
        HttpSource::new(
            "Live",
            &format!("http://127.0.0.1:{}/live.m3u8", self.port),
            NetworkScope::PinnedAddress {
                address: std::net::Ipv4Addr::LOCALHOST.into(),
            },
        )
    }

    fn requested(&self) -> Vec<String> {
        self.paths
            .lock()
            .map(|paths| paths.clone())
            .unwrap_or_default()
    }
}

const PLAYLIST: &str = "application/vnd.apple.mpegurl";

fn window(first: u64, count: u64, discontinuity_at: Option<u64>) -> Vec<u8> {
    let mut body = format!(
        "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:{first}\n"
    );
    for sequence in first..first + count {
        if discontinuity_at == Some(sequence) {
            body.push_str("#EXT-X-DISCONTINUITY\n");
        }
        let _ = write!(body, "#EXTINF:1.0,\ns{sequence}.aac\n");
    }
    body.into_bytes()
}

fn segment(path: &str, kind: &'static str) -> (u16, &'static str, Vec<u8>) {
    let name = path.trim_start_matches("/s").trim_end_matches(".aac");
    (200, kind, format!("seg{name}|").into_bytes())
}

async fn record_live(
    fixture: &Fixture,
    bytes: u64,
    seconds: Duration,
) -> (Result<live::LiveOutcome, Error>, String) {
    let directory = match tempfile::tempdir() {
        Ok(directory) => directory,
        Err(error) => return (Err(error.into()), String::new()),
    };
    let path = directory.path().join("capture.part");
    let run = async {
        let mut file = tokio::fs::File::create(&path).await?;
        let (_stop, mut signal) = tokio::sync::watch::channel(false);
        let source = fixture.source()?;
        live::record(
            &HttpAcquirer::default(),
            &source,
            AcquisitionLimits::new(bytes, seconds)?,
            &mut file,
            &mut signal,
        )
        .await
    };
    let outcome = run.await;
    let written = std::fs::read(&path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    (outcome, written)
}

fn contiguous(written: &str, first: u64) -> Option<u64> {
    let mut expected = first;
    for piece in written.split_terminator('|') {
        if piece != format!("seg{expected}") {
            return None;
        }
        expected += 1;
    }
    Some(expected - first)
}

#[tokio::test]
async fn live_playlist_advances_once_per_sequence_until_the_time_ceiling() -> TestResult {
    let fixture = Fixture::start(Arc::new(|path, count| {
        if path == "/live.m3u8" {
            let first = u64::try_from(count).unwrap_or(0);
            (200, PLAYLIST, window(first, 3, None))
        } else {
            segment(path, "audio/aac")
        }
    }))
    .await?;
    let (outcome, written) = record_live(&fixture, 1024 * 1024, Duration::from_millis(2600)).await;
    let outcome = outcome?;
    assert_eq!(outcome.gap, None);
    assert_eq!(outcome.receipt.end, TransferEnd::DurationLimit);
    assert_eq!(outcome.receipt.declared_content_type, AudioContentType::Aac);
    let fetched = contiguous(&written, 0).ok_or("segments out of order or repeated")?;
    assert!(fetched >= 4, "{written}");
    assert_eq!(outcome.receipt.bytes, u64::try_from(written.len())?);
    let requested = fixture.requested();
    let segments: Vec<_> = requested
        .iter()
        .filter(|path| path.starts_with("/s"))
        .collect();
    assert_eq!(u64::try_from(segments.len())?, fetched, "{requested:?}");
    Ok(())
}

async fn gap_case(
    reload: fn(usize) -> (u16, &'static str, Vec<u8>),
    third_kind: &'static str,
) -> std::result::Result<(Option<LiveGap>, String), Box<dyn std::error::Error>> {
    let fixture = Fixture::start(Arc::new(move |path, count| {
        if path == "/live.m3u8" {
            if count == 0 {
                (200, PLAYLIST, window(0, 3, None))
            } else {
                reload(count)
            }
        } else if path == "/s3.aac" {
            segment(path, third_kind)
        } else {
            segment(path, "audio/aac")
        }
    }))
    .await?;
    let (outcome, written) = record_live(&fixture, 1024 * 1024, Duration::from_secs(8)).await;
    let outcome = outcome?;
    assert_eq!(outcome.receipt.end, TransferEnd::EndOfBody);
    Ok((outcome.gap, written))
}

#[tokio::test]
async fn skipped_sequences_discontinuities_and_failed_reloads_end_with_a_gap() -> TestResult {
    let (gap, written) = gap_case(|_| (200, PLAYLIST, window(6, 3, None)), "audio/aac").await?;
    assert_eq!(gap, Some(LiveGap::SequenceSkip));
    assert_eq!(written, "seg0|seg1|seg2|");
    let (gap, written) = gap_case(|_| (200, PLAYLIST, window(1, 3, Some(3))), "audio/aac").await?;
    assert_eq!(gap, Some(LiveGap::Discontinuity));
    assert_eq!(written, "seg0|seg1|seg2|");
    let (gap, written) = gap_case(|_| (404, "text/plain", Vec::new()), "audio/aac").await?;
    assert_eq!(gap, Some(LiveGap::ReloadFailure));
    assert_eq!(written, "seg0|seg1|seg2|");
    let (gap, written) = gap_case(
        |_| {
            (
                200,
                PLAYLIST,
                b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv\n".to_vec(),
            )
        },
        "audio/aac",
    )
    .await?;
    assert_eq!(gap, Some(LiveGap::ReloadFailure));
    assert_eq!(written, "seg0|seg1|seg2|");
    let (gap, written) = gap_case(|_| (200, PLAYLIST, window(1, 3, None)), "audio/mpeg").await?;
    assert_eq!(gap, Some(LiveGap::CodecChange));
    assert_eq!(written, "seg0|seg1|seg2|");
    let (gap, written) = gap_case(|_| (200, PLAYLIST, window(1, 3, None)), "text/html").await?;
    assert_eq!(gap, Some(LiveGap::SegmentFailure));
    assert_eq!(written, "seg0|seg1|seg2|");
    Ok(())
}

#[tokio::test]
async fn a_stalled_playlist_ends_as_a_reload_failure() -> TestResult {
    let fixture = Fixture::start(Arc::new(|path, _| {
        if path == "/live.m3u8" {
            (200, PLAYLIST, window(0, 3, None))
        } else {
            segment(path, "audio/aac")
        }
    }))
    .await?;
    let (outcome, written) = record_live(&fixture, 1024 * 1024, Duration::from_secs(10)).await;
    let outcome = outcome?;
    assert_eq!(outcome.gap, Some(LiveGap::ReloadFailure));
    assert_eq!(written, "seg0|seg1|seg2|");
    let reloads = fixture
        .requested()
        .iter()
        .filter(|path| path.as_str() == "/live.m3u8")
        .count();
    assert!((3..=6).contains(&reloads), "{reloads}");
    Ok(())
}

#[tokio::test]
async fn transport_streams_and_whole_segment_byte_limits() -> TestResult {
    let fixture = Fixture::start(Arc::new(|path, _| {
        if path == "/live.m3u8" {
            (200, PLAYLIST, window(10, 3, None))
        } else {
            segment(path, "video/MP2T")
        }
    }))
    .await?;
    let (outcome, written) = record_live(&fixture, 13, Duration::from_secs(5)).await;
    let outcome = outcome?;
    assert_eq!(
        outcome.receipt.declared_content_type,
        AudioContentType::MpegTs
    );
    assert_eq!(outcome.receipt.end, TransferEnd::ByteLimit);
    assert_eq!(outcome.gap, None);
    assert_eq!(written, "seg10|seg11|");
    assert_eq!(outcome.receipt.bytes, 12);
    Ok(())
}

#[tokio::test]
async fn unusable_first_documents_fail_before_any_segment() -> TestResult {
    for (body, message) in [
        (
            b"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\ns0.aac\n#EXT-X-ENDLIST\n".to_vec(),
            "HLS playlist has ended; record it without --live",
        ),
        (
            b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv.m3u8\n".to_vec(),
            "HLS master playlist; resolve it and accept one variant",
        ),
    ] {
        let fixture = Fixture::start(Arc::new(move |_, _| (200, PLAYLIST, body.clone()))).await?;
        let (outcome, written) = record_live(&fixture, 1024, Duration::from_secs(5)).await;
        assert!(
            matches!(&outcome, Err(Error::InvalidInput(text)) if *text == message),
            "{outcome:?}"
        );
        assert!(written.is_empty());
        assert_eq!(fixture.requested(), ["/live.m3u8"]);
    }
    let fixture = Fixture::start(Arc::new(|path, _| {
        if path == "/live.m3u8" {
            (200, PLAYLIST, window(0, 1, None))
        } else {
            (200, "video/mp4", b"not audio".to_vec())
        }
    }))
    .await?;
    let (outcome, written) = record_live(&fixture, 1024, Duration::from_secs(5)).await;
    assert!(matches!(
        outcome,
        Err(Error::Acquisition("unsupported audio content type"))
    ));
    assert!(written.is_empty());
    Ok(())
}

#[test]
fn a_repeated_codecs_claim_is_unknown_but_other_repeats_stay_malformed() -> TestResult {
    // Shape observed on a national broadcaster's master playlist on 2026-09-24.
    let entries = master(
        b"#EXTM3U\n#EXT-X-STREAM-INF:PROGRAM-ID=1,AVERAGE-BANDWIDTH=32000,BANDWIDTH=32000,CODECS=\"mp4a.40.29\",CODECS=\"mp4a.40.5\"\nlow.m3u8\n#EXT-X-STREAM-INF:PROGRAM-ID=1,AVERAGE-BANDWIDTH=64000,BANDWIDTH=64000,CODECS=\"mp4a.40.29\",CODECS=\"mp4a.40.5\"\nhigh.m3u8\n",
    )?;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].bandwidth, Some(32_000));
    assert_eq!(entries[0].codecs, None);
    assert_eq!(entries[0].audio_only, None);
    assert!(
        master(b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=32000,BANDWIDTH=64000\nlow.m3u8\n").is_err()
    );
    Ok(())
}
