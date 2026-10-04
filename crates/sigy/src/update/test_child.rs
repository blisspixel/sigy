use std::{
    io::Read,
    ops::{Deref, DerefMut},
    process::{Child, Output},
};

pub(super) struct OwnedChild(Child);

impl From<Child> for OwnedChild {
    fn from(child: Child) -> Self {
        Self(child)
    }
}

impl Deref for OwnedChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}

impl DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

impl OwnedChild {
    pub(super) fn output(&mut self) -> std::io::Result<Output> {
        let status = self.0.wait()?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        if let Some(mut pipe) = self.0.stdout.take() {
            pipe.read_to_end(&mut stdout)?;
        }
        if let Some(mut pipe) = self.0.stderr.take() {
            pipe.read_to_end(&mut stderr)?;
        }
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
