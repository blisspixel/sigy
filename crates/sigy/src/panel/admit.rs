//! Play admission. These checks refuse work before a listen starts.

use crate::explorer::text::sanitize;

pub(crate) const LANGUAGE: &str =
    "Recognition and translation are not part of this panel. No language is qualified.";
pub(crate) const AUDIO_NOTE: &str = "Device follow is none. The next explicit play uses the current default output. Acoustic delivery is not qualified.";
pub(crate) const EXPLORER: &str = "sigy tui";
pub(crate) const STDIN_CLOSED: &str = "stdin is already closed. Refusing to start a listen. Omit --cancel-on-stdin, or hold stdin open for the whole play";
pub(crate) const PARENT_GONE: &str = "the parent process exited before playback started";
pub(crate) const SESSION_UNREADABLE: &str = "panel session hint is unreadable. Inspect panel-session.json in the library before starting another listen";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceChoice {
    pub kind: String,
    pub revision_id: String,
}

#[must_use]
pub(crate) fn service_required() -> String {
    "the background service is not running. Start it with `sigy service start`. Starting the service can resume a saved directory policy and monitor processing. This panel will not start it".into()
}

#[must_use]
pub(crate) fn service_stopping() -> String {
    "the background service is stopping. Wait until it exits, then start it again with `sigy service start` if you still want playback. Starting the service can resume a saved directory policy and monitor processing. This panel will not start it".into()
}

/// # Errors
/// Returns the next command when the favorite cannot be played directly.
#[allow(clippy::fn_params_excessive_bools)]
pub(crate) fn admit(
    station_id: &str,
    cached: bool,
    favorite: bool,
    hls: bool,
    sources: &[SourceChoice],
    more_sources: bool,
) -> Result<String, String> {
    let station = sanitize(station_id, 128);
    if !cached {
        return Err(format!(
            "that station is not in the cached directory. Search with `sigy radio search` and save a favorite with `sigy radio favorite {station}`"
        ));
    }
    if !favorite {
        return Err(format!(
            "that station is not a cached favorite. Save it with `sigy radio favorite {station}`, or list favorites with `sigy radio search --favorites`"
        ));
    }
    if hls {
        return Err(format!(
            "this favorite is an HLS listing. Direct listen does not play a master playlist. Inspect it with `sigy radio linked {station}`"
        ));
    }
    if more_sources || sources.len() != 1 {
        return Err(linked_refusal(&station));
    }
    let Some(source) = sources.first() else {
        return Err(linked_refusal(&station));
    };
    if source.kind != "http_audio" || source.revision_id.is_empty() {
        return Err(linked_refusal(&station));
    }
    Ok(source.revision_id.clone())
}

fn linked_refusal(station: &str) -> String {
    format!(
        "this favorite does not have exactly one registered audio revision. Inspect `sigy radio linked {station}` and play one revision with `sigy listen source`"
    )
}

#[must_use]
pub(crate) fn already_running(id: &str) -> String {
    let id = sanitize(id, 128);
    format!(
        "a panel listen is already running: {id}. Stop it with `sigy panel stop --id {id}` before starting another"
    )
}

#[must_use]
pub(crate) fn reused_listen(id: &str) -> String {
    let id = sanitize(id, 128);
    format!(
        "listen ID {id} is already used and was not started again. Choose a new listen ID. This panel did not stop the existing listen"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_names_the_next_command_and_accepts_one_audio_revision() -> Result<(), String> {
        let audio = SourceChoice {
            kind: "http_audio".into(),
            revision_id: "rev-1".into(),
        };
        assert_eq!(
            admit(
                "station",
                true,
                true,
                false,
                std::slice::from_ref(&audio),
                false
            )
            .ok(),
            Some("rev-1".into())
        );
        let missing = admit(
            "station",
            false,
            true,
            false,
            std::slice::from_ref(&audio),
            false,
        )
        .err()
        .ok_or("missing station")?;
        assert!(missing.contains("sigy radio search"));
        let plain = admit(
            "station",
            true,
            false,
            false,
            std::slice::from_ref(&audio),
            false,
        )
        .err()
        .ok_or("not favorite")?;
        assert!(plain.contains("sigy radio favorite"));
        let hls = admit(
            "station",
            true,
            true,
            true,
            std::slice::from_ref(&audio),
            false,
        )
        .err()
        .ok_or("hls")?;
        assert!(hls.contains("sigy radio linked"));
        let extra = admit(
            "station",
            true,
            true,
            false,
            std::slice::from_ref(&audio),
            true,
        )
        .err()
        .ok_or("more sources")?;
        assert!(extra.contains("sigy listen source"));
        let none = admit("station", true, true, false, &[], false)
            .err()
            .ok_or("no revision")?;
        assert!(none.contains("sigy radio linked"));
        let other = SourceChoice {
            kind: "other".into(),
            revision_id: "rev-1".into(),
        };
        assert!(admit("station", true, true, false, &[other], false).is_err());
        assert!(service_required().contains("sigy service start"));
        assert!(service_required().contains("resume"));
        assert!(already_running("listen\u{1b}[2J").contains("sigy panel stop --id listen"));
        assert!(
            !already_running("listen\u{1b}[2J")
                .chars()
                .any(char::is_control)
        );
        assert!(reused_listen("old").contains("Choose a new listen ID"));
        Ok(())
    }
}
