//! The process table: which agents are alive, which harness process a hook
//! came from, and which ancestor owns the terminal window.

use std::path::{Path, PathBuf};

use agentscope_adapters::{Adapter, ProcessInfo};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// How far up the process tree to look for a terminal or harness.
const MAX_DEPTH: usize = 16;

pub struct Procs {
    sys: System,
}

fn refresh_kind() -> ProcessRefreshKind {
    // Threads are listed as processes on Linux unless tasks are left out.
    ProcessRefreshKind::nothing()
        .without_tasks()
        .with_cmd(UpdateKind::OnlyIfNotSet)
        .with_exe(UpdateKind::OnlyIfNotSet)
        .with_cwd(UpdateKind::Always)
}

impl Procs {
    pub fn new() -> Self {
        Self { sys: System::new() }
    }

    /// Re-reads the whole table. Runs on the sweep interval.
    pub fn refresh(&mut self) {
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind());
    }

    /// Refreshes just `pids`, for lookups between sweeps.
    fn refresh_some(&mut self, pids: &[u32]) {
        let pids: Vec<Pid> = pids.iter().map(|p| Pid::from_u32(*p)).collect();
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::Some(&pids), true, refresh_kind());
    }

    pub fn info(&self, pid: u32) -> Option<ProcessInfo> {
        let p = self.sys.process(Pid::from_u32(pid))?;
        Some(ProcessInfo {
            pid,
            ppid: p.parent().map(|p| p.as_u32()),
            name: p.name().to_string_lossy().into_owned(),
            cmd: p
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
            cwd: p.cwd().map(Path::to_path_buf),
        })
    }

    pub fn exe(&self, pid: u32) -> Option<PathBuf> {
        self.sys
            .process(Pid::from_u32(pid))?
            .exe()
            .map(Path::to_path_buf)
    }

    /// `pid` and its ancestors, nearest first.
    pub fn ancestors(&mut self, pid: u32) -> Vec<ProcessInfo> {
        let mut chain = Vec::new();
        let mut next = Some(pid);
        while let Some(pid) = next.filter(|_| chain.len() < MAX_DEPTH) {
            if self.sys.process(Pid::from_u32(pid)).is_none() {
                self.refresh_some(&[pid]);
            }
            let Some(info) = self.info(pid) else { break };
            next = info.ppid.filter(|p| *p != pid && *p != 0);
            chain.push(info);
        }
        chain
    }

    /// Whether `pid` is still a process of `adapter`'s harness. Checking the
    /// name guards against the pid being reused by something else.
    pub fn is_agent(&self, pid: u32, adapter: &dyn Adapter) -> bool {
        self.info(pid)
            .is_some_and(|p| adapter.match_process(&p).is_some())
    }

    /// The harness process a hook shim ran under: the nearest ancestor of the
    /// shim's parent that the adapter recognizes.
    pub fn harness_pid(&mut self, ppid: u32, adapter: &dyn Adapter) -> Option<u32> {
        self.ancestors(ppid)
            .into_iter()
            .find(|p| adapter.match_process(p).is_some())
            .map(|p| p.pid)
    }

    /// A running harness process whose cwd is `cwd`, for sessions no hook
    /// has reported a pid for.
    pub fn find_by_cwd(&self, cwd: &Path, adapter: &dyn Adapter) -> Option<u32> {
        self.sys.processes().iter().find_map(|(pid, p)| {
            if p.cwd() != Some(cwd) {
                return None;
            }
            let info = self.info(pid.as_u32())?;
            adapter.match_process(&info).map(|_| pid.as_u32())
        })
    }
}
