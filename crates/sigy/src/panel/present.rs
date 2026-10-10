//! Text and Waybar projections share one read-only view.

use serde_json::{Value, json};

use super::admit::{AUDIO_NOTE, EXPLORER, LANGUAGE};
use crate::explorer::text::sanitize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServicePresence {
    Absent,
    Running,
    Stopping,
}

impl ServicePresence {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Running => "running",
            Self::Stopping => "stopping",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Freshness {
    Empty,
    Stale,
    Current,
}

impl Freshness {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Stale => "stale",
            Self::Current => "current",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FavoriteRow {
    pub id: String,
    pub name: String,
    pub country: String,
    pub hls: bool,
    pub codec: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordingRow {
    pub id: String,
    pub state: String,
    pub storage_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListenRow {
    pub id: String,
    pub state: String,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Projection {
    pub service: ServicePresence,
    pub freshness: Freshness,
    pub cached_stations: u32,
    pub favorite_stations: u32,
    pub stale_stations: u32,
    pub newest_age: Option<String>,
    pub favorites: Vec<FavoriteRow>,
    pub favorites_truncated: bool,
    pub active_captures: u64,
    pub recordings: Vec<RecordingRow>,
    pub recordings_truncated: bool,
    pub listen: Option<ListenRow>,
    pub session_hint: &'static str,
}

#[must_use]
pub(crate) fn bar_class(view: &Projection) -> &'static str {
    if view.service != ServicePresence::Running {
        return "offline";
    }
    if view
        .listen
        .as_ref()
        .is_some_and(|listen| matches!(listen.state.as_str(), "failed" | "interrupted"))
    {
        return "failed";
    }
    if view
        .listen
        .as_ref()
        .is_some_and(|listen| listen.state == "running")
    {
        return "playing";
    }
    if view.active_captures > 0 {
        return "recording";
    }
    if view.freshness != Freshness::Current {
        return "stale";
    }
    "idle"
}

#[must_use]
pub(crate) fn text(view: &Projection) -> String {
    let mut lines = vec![
        "World-radio panel. Read only. This is not a qualified installation.".into(),
        format!("Service: {}.", view.service.as_str()),
        format!(
            "Directory: {}. Cached {}, favorites {}, stale {}{}.",
            view.freshness.as_str(),
            view.cached_stations,
            view.favorite_stations,
            view.stale_stations,
            view.newest_age
                .as_deref()
                .map(|age| format!(", newest {age}"))
                .unwrap_or_default()
        ),
    ];
    if view.favorites.is_empty() {
        lines.push("Favorites: none shown.".into());
    } else {
        lines.push("Favorites:".into());
        for favorite in &view.favorites {
            lines.push(format!(
                "  {} | {} | {} | {} | hls {}",
                clean(&favorite.id),
                clean(&favorite.name),
                clean(&favorite.country),
                clean(&favorite.codec),
                if favorite.hls { "yes" } else { "no" }
            ));
        }
    }
    if view.favorites_truncated {
        lines.push("Favorites page is partial. `sigy radio search --favorites` lists more.".into());
    }
    lines.push(format!(
        "Recordings: {} active. {} visible row{}.",
        view.active_captures,
        view.recordings.len(),
        if view.recordings.len() == 1 { "" } else { "s" }
    ));
    for recording in &view.recordings {
        lines.push(format!(
            "  {} | {} | {}",
            clean(&recording.id),
            clean(&recording.state),
            clean(&recording.storage_state)
        ));
    }
    if view.recordings_truncated {
        lines.push("Recording page is partial. `sigy record list` shows the rest.".into());
    }
    match &view.listen {
        Some(listen) => {
            lines.push(format!(
                "Listen {}: {}.",
                clean(&listen.id),
                clean(&listen.state)
            ));
            if let Some(failure) = &listen.failure {
                lines.push(format!("Reason: {}.", clean(failure)));
            }
        }
        None => lines.push("Listen: none.".into()),
    }
    lines.push(format!("Session hint: {}.", view.session_hint));
    lines.push(LANGUAGE.into());
    lines.push(AUDIO_NOTE.into());
    lines.push(format!("Explorer: {EXPLORER}."));
    lines.join("\n")
}

#[must_use]
pub(crate) fn status_json(view: &Projection) -> Value {
    json!({
        "qualified_platform": false,
        "qualified_linux": false,
        "qualified_omarchy": false,
        "read_only": true,
        "service": view.service.as_str(),
        "directory": {
            "freshness": view.freshness.as_str(),
            "cached_stations": view.cached_stations,
            "favorite_stations": view.favorite_stations,
            "stale_stations": view.stale_stations,
            "newest_age": view.newest_age.as_deref().map(clean),
        },
        "favorites": view.favorites.iter().map(favorite_json).collect::<Vec<_>>(),
        "favorites_truncated": view.favorites_truncated,
        "active_captures": view.active_captures,
        "recordings": view.recordings.iter().map(recording_json).collect::<Vec<_>>(),
        "recordings_truncated": view.recordings_truncated,
        "listen": view.listen.as_ref().map(listen_json),
        "session_hint": view.session_hint,
        "language": LANGUAGE,
        "audio_note": AUDIO_NOTE,
        "explorer": EXPLORER,
    })
}

#[must_use]
pub(crate) fn bar_json(view: &Projection) -> Value {
    let class = bar_class(view);
    json!({
        "text": class,
        "tooltip": text(view),
        "class": class,
    })
}

fn favorite_json(favorite: &FavoriteRow) -> Value {
    json!({
        "id": clean(&favorite.id),
        "name": clean(&favorite.name),
        "country": clean(&favorite.country),
        "hls": favorite.hls,
        "codec": clean(&favorite.codec),
    })
}

fn recording_json(recording: &RecordingRow) -> Value {
    json!({
        "id": clean(&recording.id),
        "state": clean(&recording.state),
        "storage_state": clean(&recording.storage_state),
    })
}

fn listen_json(listen: &ListenRow) -> Value {
    json!({
        "id": clean(&listen.id),
        "state": clean(&listen.state),
        "failure": listen.failure.as_deref().map(clean),
    })
}

fn clean(value: &str) -> String {
    sanitize(value, 256)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> Projection {
        Projection {
            service: ServicePresence::Running,
            freshness: Freshness::Current,
            cached_stations: 2,
            favorite_stations: 1,
            stale_stations: 0,
            newest_age: Some("1h".into()),
            favorites: vec![FavoriteRow {
                id: "station".into(),
                name: "News\u{1b}[2J".into(),
                country: "CA".into(),
                hls: false,
                codec: "MP3".into(),
            }],
            favorites_truncated: false,
            active_captures: 0,
            recordings: Vec::new(),
            recordings_truncated: false,
            listen: None,
            session_hint: "absent",
        }
    }

    #[test]
    fn text_and_bar_share_priority_and_strip_controls() -> Result<(), Box<dyn std::error::Error>> {
        let mut projection = view();
        let rendered = text(&projection);
        assert!(rendered.contains(LANGUAGE));
        assert!(rendered.contains("not a qualified installation"));
        assert!(rendered.contains("News"));
        assert!(!rendered.contains('\u{1b}'));
        assert!(!rendered.contains("stream_origin"));
        assert_eq!(bar_class(&projection), "idle");
        let bar = bar_json(&projection);
        assert_eq!(bar["class"], "idle");
        assert_eq!(bar["text"], "idle");
        assert!(
            bar["tooltip"]
                .as_str()
                .is_some_and(|tip| tip.contains(LANGUAGE))
        );

        projection.freshness = Freshness::Empty;
        assert_eq!(bar_class(&projection), "stale");
        projection.freshness = Freshness::Stale;
        projection.active_captures = 1;
        assert_eq!(bar_class(&projection), "recording");
        projection.listen = Some(ListenRow {
            id: "listen".into(),
            state: "running".into(),
            failure: None,
        });
        assert_eq!(bar_class(&projection), "playing");
        let listen = projection.listen.as_mut().ok_or("listen")?;
        listen.state = "failed".into();
        listen.failure = Some("gone\u{1b}[31m".into());
        assert_eq!(bar_class(&projection), "failed");
        assert!(
            !text(&projection)
                .chars()
                .any(|ch| ch.is_control() && ch != '\n')
        );
        projection.service = ServicePresence::Stopping;
        assert_eq!(bar_class(&projection), "offline");
        projection.service = ServicePresence::Absent;
        let document = status_json(&projection);
        assert_eq!(document["qualified_platform"], false);
        assert_eq!(document["qualified_linux"], false);
        assert_eq!(document["qualified_omarchy"], false);
        assert_eq!(document["read_only"], true);
        assert_eq!(document["service"], "absent");
        assert_eq!(document["favorites"][0]["name"], "News[2J");
        assert!(document.get("stream_origin").is_none());
        assert_eq!(document["language"], LANGUAGE);
        Ok(())
    }
}
