use agentscope_core::{AgentEvent, HarnessId, RawHookEvent};

use crate::*;

pub struct CopilotCli;

impl Adapter for CopilotCli {
    fn id(&self) -> HarnessId {
        HarnessId::CopilotCli
    }

    fn detect(&self) -> Detection {
        detect_config_dir(self.id(), home_dir(".copilot"))
    }

    fn hooks_status(&self) -> HookStatus {
        HookStatus::NotInstalled
    }

    fn install_hooks(&self, _plan: &HookPlan) -> Result<Diff> {
        Err(AdapterError::NotImplemented("hook install"))
    }

    fn uninstall_hooks(&self) -> Result<Diff> {
        Err(AdapterError::NotImplemented("hook uninstall"))
    }

    fn normalize_hook(&self, _raw: &RawHookEvent) -> Vec<AgentEvent> {
        Vec::new()
    }

    fn transcript_roots(&self) -> Vec<WatchRoot> {
        Vec::new()
    }

    fn parse_transcript_chunk(&self, _file: &TranscriptFile, _bytes: &[u8]) -> Vec<AgentEvent> {
        Vec::new()
    }

    fn match_process(&self, p: &ProcessInfo) -> Option<ProcessMatch> {
        match_by_name(self.id(), &["copilot"], p)
    }
}
