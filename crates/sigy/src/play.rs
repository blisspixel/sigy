//! Explicit quick listening composes the existing directory and listen operations.

use crate::panel::{
    admit,
    session::{self, PanelSession, SessionHint},
};
use sigy_service::{
    control::{self, DirectoryOperation, Operation},
    recordings::{PlaybackDestination, audio::NativeAudioGroup},
    sources::RedirectPolicy,
};
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StationMode {
    Favorite,
    Direct,
}

pub(crate) async fn revision(directory: &Path, station: &str, mode: StationMode) -> Result<String> {
    let shown = control::request(
        directory,
        DirectoryOperation::Show {
            id: station.to_owned(),
        }
        .into(),
    )
    .await?;
    let page = shown.station_page.ok_or("station metadata missing")?;
    let row = page.entries.first().ok_or("station metadata missing")?;
    let favorite = mode == StationMode::Direct || page.favorite_ids.iter().any(|id| id == station);
    let linked = control::request(
        directory,
        DirectoryOperation::Linked {
            id: station.to_owned(),
            catalog: None,
        }
        .into(),
    )
    .await?;
    let context = linked
        .linked_station_context
        .ok_or("linked station context missing")?;
    if mode == StationMode::Direct
        && context.cached
        && !row.hls
        && context.sources.is_empty()
        && !context.more_sources
    {
        let revision = format!("station-{station}:v1");
        control::request(
            directory,
            DirectoryOperation::Add {
                id: station.to_owned(),
                revision_id: revision.clone(),
                redirects: RedirectPolicy::Deny,
            }
            .into(),
        )
        .await?;
        eprintln!("Registered audio revision {revision}. Redirects are denied.");
        return Ok(revision);
    }
    let sources = context
        .sources
        .iter()
        .map(|source| admit::SourceChoice {
            kind: source.source.kind.clone(),
            revision_id: source.source.revision_id.clone(),
        })
        .collect::<Vec<_>>();
    Ok(admit::admit(
        station,
        context.cached,
        favorite,
        row.hls,
        &sources,
        context.more_sources,
    )?)
}

pub(crate) async fn execute(
    directory: &Path,
    station: &str,
    destination: &str,
    parent_pid: Option<u32>,
    cancel_on_stdin: bool,
    json: bool,
) -> Result<()> {
    let destination = PlaybackDestination::parse(destination)?;
    let status = control::request(directory, Operation::Status {})
        .await
        .map_err(|_| admit::service_required())?;
    if status
        .service
        .as_ref()
        .is_none_or(|service| service.stopping)
    {
        return Err(admit::service_stopping().into());
    }
    if parent_pid.is_some_and(|pid| !crate::panel::is_process_alive(pid)) {
        return Err(admit::PARENT_GONE.into());
    }
    let cancel = crate::panel::setup_cancel(cancel_on_stdin, parent_pid).await?;
    match session::read(directory) {
        SessionHint::Unreadable => return Err(admit::SESSION_UNREADABLE.into()),
        SessionHint::Present(hint) => {
            let snapshot = control::request(
                directory,
                Operation::Listen {
                    command: sigy_service::control::ListenOperation::Status {
                        id: hint.listen_id.clone(),
                    },
                },
            )
            .await?;
            if snapshot
                .listen
                .is_some_and(|listen| listen.state == "running")
            {
                return Err(admit::already_running(&hint.listen_id).into());
            }
        }
        SessionHint::Absent => (),
    }
    let _decoder = crate::listen::decoder_path(directory).await?;
    let _group = NativeAudioGroup::for_decoder()?;
    let revision_id = revision(directory, station, StationMode::Direct).await?;
    let id = crate::radio::request_id("play")?;
    let hint = PanelSession {
        listen_id: id.clone(),
        station_id: station.to_owned(),
        revision_id: revision_id.clone(),
        state: "starting".into(),
    };
    session::write(directory, &hint)?;
    let command = format!(
        "sigy --data-dir {}",
        crate::radio::quote(&directory.display().to_string())
    );
    eprintln!(
        "Listen {id} | revision {revision_id}. Stop with `{command} stop` or `{command} listen stop {id}`."
    );
    let result = async {
        match crate::listen::start_listen(directory, &id, &revision_id).await? {
            crate::listen::ListenStart::Existing(_) => return Err(admit::reused_listen(&id).into()),
            crate::listen::ListenStart::New => (),
        }
        let stop = async {
            tokio::select! { () = cancel => (), _ = tokio::signal::ctrl_c() => (), }
        };
        crate::listen::finish_new_listen(directory, &id, destination, stop).await
    }
    .await;
    match result {
        Ok(played) => {
            session::settle(directory, &id, &played.finished.state)?;
            crate::listen::write_new_listen(
                &mut std::io::stdout().lock(),
                &id,
                &revision_id,
                destination,
                &played,
                json,
            )
        }
        Err(error) => {
            let _ = session::settle(directory, &id, "failed");
            Err(error)
        }
    }
}
