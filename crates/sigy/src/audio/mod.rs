//! Child-local audio output. It carries no library or network authority.

#[cfg(windows)]
mod backend;
mod helper;
#[cfg(any(windows, test))]
pub(crate) mod pcm;
#[cfg(windows)]
pub(crate) mod play;
pub(crate) mod protocol;

pub(crate) use helper::run_helper;

type Failure = Box<dyn std::error::Error>;

#[cfg(test)]
mod tests;
