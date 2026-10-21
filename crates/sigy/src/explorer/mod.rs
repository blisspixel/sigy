//! List explorer. The client uses the existing catalog owner and leaves on quit.

mod client;
pub(crate) mod context;
mod country;
mod fetch;
mod finding;
mod globe;
mod metadata;
mod monitor;
mod player;
mod screen;
mod search;
mod state;
mod terminal;
pub(crate) mod text;
mod timeline;

pub(crate) use globe::utc_label;

use std::time::{SystemTime, UNIX_EPOCH};
use std::{io::Write, path::Path};

pub use state::{Emphasis, Modes};

use state::Explorer;

pub fn run(
    data_dir: &Path,
    presentation: Modes,
    inspect: Option<&Path>,
    filter: sigy_service::discovery::StationFilter,
) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()?;
    let now = now_ms()?;
    let mut model = Explorer::new(presentation, now);
    model.initialize_search(filter, data_dir)?;
    let desk = runtime.block_on(client::load_desk(data_dir, &model.current_search(), now))?;
    model.apply_desk(desk);
    terminal::drive(&runtime, data_dir, &mut model, inspect)?;
    if inspect.is_none() && model.rows().is_empty() {
        writeln!(
            std::io::stdout().lock(),
            "Current filters: {}",
            model.discovery_command()
        )?;
    }
    Ok(())
}

fn now_ms() -> Result<i64, Box<dyn std::error::Error>> {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    i64::try_from(millis).map_err(|_| "clock is out of range".into())
}
