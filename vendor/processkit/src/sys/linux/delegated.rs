//! Explicit delegated parent and separate native process/thread policy.

use super::*;
use seccompiler::{BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter, SeccompRule};

pub(super) struct Policy {
    pub(super) admission: Mutex<()>,
    pub(super) filters: Vec<BpfProgram>,
    processes: u32,
    tasks: u32,
}

impl Policy {
    pub(super) fn prepare_leaf(&self, leaf: &LeafSlot<'_>) -> io::Result<()> {
        let path = leaf.dir.as_ref().ok_or_else(|| io::Error::other("native task leaf unavailable"))?;
        // Migration itself bypasses pids.max. Give every admitted process its own
        // fixed share before joining, so concurrently created threads cannot fill
        // the parent and let a second migration exceed the aggregate task cap.
        let expected = (self.tasks / self.processes).to_string();
        cgroup_write(&path.join("pids.max"), &expected)?;
        if std::fs::read_to_string(path.join("pids.max"))?.trim() != expected {
            return Err(io::Error::other("native leaf task limit readback mismatch"));
        }
        Ok(())
    }

    pub(super) fn check_capacity(&self, group: &Cgroup) -> io::Result<()> {
        let tasks: u32 = std::fs::read_to_string(group.path.join("pids.current"))?.trim().parse()
            .map_err(|_| io::Error::other("invalid native task count"))?;
        if group.members()?.len() >= self.processes as usize || tasks >= self.tasks {
            return Err(io::Error::from_raw_os_error(libc::EAGAIN));
        }
        Ok(())
    }
}

fn compile(rules: std::collections::BTreeMap<i64, Vec<SeccompRule>>, errno: i32) -> io::Result<BpfProgram> {
    SeccompFilter::new(rules, SeccompAction::Allow, SeccompAction::Errno(errno as u32),
        std::env::consts::ARCH.try_into().map_err(io::Error::other)?)
        .map_err(io::Error::other)?.try_into().map_err(io::Error::other)
}

fn filters() -> io::Result<Vec<BpfProgram>> {
    // clone without CLONE_THREAD creates a new process. clone3 arguments are
    // indirect; ENOSYS requests the libc clone fallback for ordinary threads.
    let condition = SeccompCondition::new(0, SeccompCmpArgLen::Qword,
        SeccompCmpOp::MaskedEq(libc::CLONE_THREAD as u64), 0).map_err(io::Error::other)?;
    let rule = SeccompRule::new(vec![condition]).map_err(io::Error::other)?;
    let mut process = std::collections::BTreeMap::from([(libc::SYS_clone, vec![rule])]);
    #[cfg(target_arch = "x86_64")]
    {
        process.insert(libc::SYS_fork, vec![]);
        process.insert(libc::SYS_vfork, vec![]);
        process.insert(libc::SYS_clone | 0x4000_0000, vec![]);
        process.insert(libc::SYS_fork | 0x4000_0000, vec![]);
        process.insert(libc::SYS_vfork | 0x4000_0000, vec![]);
    }
    let mut clone3 = std::collections::BTreeMap::from([(libc::SYS_clone3, vec![])]);
    #[cfg(target_arch = "x86_64")]
    clone3.insert(libc::SYS_clone3 | 0x4000_0000, vec![]);
    Ok(vec![compile(process, libc::EPERM)?, compile(clone3, libc::ENOSYS)?])
}

#[cfg(feature = "limits")]
pub(super) fn new_job(limits: &ResourceLimits, parent: &Path, tasks: u32) -> io::Result<Job> {
    let processes = limits.max_processes.ok_or_else(|| io::Error::other("native process cap missing"))?;
    if processes == 0 || tasks < processes || tasks > 4096 || limits.max_memory.is_none() || limits.cpu_quota.is_none() {
        return Err(io::Error::other("invalid delegated native bounds"));
    }
    let policy = Policy { admission: Mutex::new(()), filters: filters()?, processes, tasks };
    let mut kernel = *limits;
    kernel.max_processes = Some(tasks);
    let mut group = Cgroup::create_in(parent, &kernel)?;
    // Readback is part of admission, not inferred from a successful write.
    let readback = || -> io::Result<()> {
        for (file, expected) in [("memory.max", limits.max_memory.unwrap().to_string()),
            ("pids.max", tasks.to_string()), ("cpu.max", cpu_max_value(limits.cpu_quota.unwrap()))] {
            if std::fs::read_to_string(group.path.join(file))?.trim() != expected {
                return Err(io::Error::other("native limit readback mismatch"));
            }
        }
        cgroup_write(&group.path.join("memory.swap.max"), "0")?;
        if std::fs::read_to_string(group.path.join("memory.swap.max"))?.trim() != "0" {
            return Err(io::Error::other("native swap limit readback mismatch"));
        }
        if !group.path.join("cgroup.kill").is_file() {
            return Err(io::Error::other("recursive native kill unavailable"));
        }
        cgroup_write(&group.path.join("cgroup.subtree_control"), "+pids")?;
        if !std::fs::read_to_string(group.path.join("cgroup.subtree_control"))?
            .split_whitespace().any(|controller| controller == "pids") {
            return Err(io::Error::other("native leaf task controller unavailable"));
        }
        Ok(())
    };
    if let Err(error) = readback() {
        let _ = std::fs::remove_dir(&group.path);
        return Err(error);
    }
    group.strict = Some(policy);
    Ok(Job { backend: Backend::Cgroup(group), skip_drop_kill: super::super::SkipDropKill::new() })
}
