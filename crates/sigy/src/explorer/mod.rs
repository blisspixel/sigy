//! List explorer. The client uses the existing catalog owner and leaves on quit.

mod client;
mod screen;
mod state;
mod terminal;
mod text;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub use state::Modes;

use state::Explorer;

pub fn run(
    data_dir: &Path,
    presentation: Modes,
    inspect: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()?;
    let now = now_ms()?;
    let mut model = Explorer::new(presentation, now);
    let desk = runtime.block_on(client::load_desk(data_dir, "", false, now))?;
    model.apply_desk(desk);
    terminal::drive(&runtime, data_dir, &mut model, inspect)
}

fn now_ms() -> Result<i64, Box<dyn std::error::Error>> {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    i64::try_from(millis).map_err(|_| "clock is out of range".into())
}
