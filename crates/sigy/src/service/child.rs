//! Platform process creation with no inherited terminal or control handles.

use std::path::Path;

use super::Result;

#[cfg(windows)]
pub struct ServiceChild(winsafe::guard::CloseHandlePiGuard);

#[cfg(windows)]
impl ServiceChild {
    pub fn spawn(directory: &Path) -> Result<Self> {
        use winsafe::{CreateProcess, STARTUPINFO, co};
        let executable = std::env::current_exe()?;
        let executable = executable
            .to_str()
            .ok_or("executable path contains invalid Unicode")?;
        let directory = directory
            .to_str()
            .ok_or("library path contains invalid Unicode")?;
        // Paths are separate API fields, never interpolated into a command line.
        // Stable Rust Command inherits unrelated inheritable Windows handles,
        // even with null stdio. Explicit FALSE prevents that propagation.
        let process = CreateProcess(
            Some(executable),
            Some("sigy --data-dir . service run --background"),
            None,
            None,
            false,
            co::CREATE::NO_WINDOW | co::CREATE::NEW_PROCESS_GROUP,
            &[],
            Some(directory),
            &mut STARTUPINFO::default(),
        )?;
        Ok(Self(process))
    }

    pub fn id(&self) -> u32 {
        self.0.dwProcessId
    }

    pub fn exited(&mut self) -> Result<Option<String>> {
        if self.0.hProcess.WaitForSingleObject(Some(0))? == winsafe::co::WAIT::OBJECT_0 {
            Ok(Some(self.0.hProcess.GetExitCodeProcess()?.to_string()))
        } else {
            Ok(None)
        }
    }

    pub fn terminate(&mut self) {
        let _ = self.0.hProcess.TerminateProcess(1);
        let _ = self.0.hProcess.WaitForSingleObject(Some(5000));
    }
}

#[cfg(unix)]
pub struct ServiceChild(std::process::Child);

#[cfg(unix)]
impl ServiceChild {
    pub fn spawn(directory: &Path) -> Result<Self> {
        use std::process::{Command, Stdio};
        let child = Command::new(std::env::current_exe()?)
            .arg("--data-dir")
            .arg(directory)
            .args(["service", "run", "--background"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Self(child))
    }

    pub fn id(&self) -> u32 {
        self.0.id()
    }

    pub fn exited(&mut self) -> Result<Option<String>> {
        Ok(self.0.try_wait()?.map(|status| status.to_string()))
    }

    pub fn terminate(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
