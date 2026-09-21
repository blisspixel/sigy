use std::{
    fs::File,
    io::{self, Read, Seek},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_secs(20);
const MAX_OUTPUT: u64 = 256 * 1024;

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn output(command: &mut Command) -> io::Result<Output> {
    // Files avoid waiting forever for EOF if a detached descendant inherits a
    // caller's pipe handle. Read only after the owned command has exited.
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = OwnedChild(
        command
            .stdin(Stdio::null())
            .stdout(stdout.try_clone()?)
            .stderr(stderr.try_clone()?)
            .spawn()?,
    );
    let deadline = Instant::now() + DEADLINE;
    loop {
        if stdout.metadata()?.len() > MAX_OUTPUT || stderr.metadata()?.len() > MAX_OUTPUT {
            return Err(io::Error::other(
                "test process exceeded its output allowance",
            ));
        }
        if let Some(status) = child.0.try_wait()? {
            return Ok(Output {
                status,
                stdout: read_output(&mut stdout)?,
                stderr: read_output(&mut stderr)?,
            });
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "test process exceeded its deadline",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn read_output(file: &mut File) -> io::Result<Vec<u8>> {
    file.rewind()?;
    let mut bytes = Vec::new();
    file.take(MAX_OUTPUT + 1).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).map_err(io::Error::other)? > MAX_OUTPUT {
        return Err(io::Error::other(
            "test process exceeded its output allowance",
        ));
    }
    Ok(bytes)
}
