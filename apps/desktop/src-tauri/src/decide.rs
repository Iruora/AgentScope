//! Permission prompts waiting for the user's answer in AgentScope.
//!
//! A `--decide` shim holds its HTTP request open while the prompt is listed
//! here. Answering sends the decision back; letting go (the user chose to
//! answer in the terminal, the window ran out, or the prompt was answered
//! elsewhere) drops the sender, the shim prints nothing, and the harness asks
//! in its own prompt as if AgentScope were not there.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use agentscope_core::Decision;
use tokio::sync::oneshot;

/// What the agent is told when the user denies from AgentScope.
pub const DENY_MESSAGE: &str = "The user denied this in AgentScope.";

#[derive(Default)]
pub struct Decisions {
    waiting: Mutex<HashMap<String, oneshot::Sender<Option<Decision>>>>,
}

impl Decisions {
    fn lock(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<String, oneshot::Sender<Option<Decision>>>> {
        self.waiting.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn register(&self, approval_id: String, reply: oneshot::Sender<Option<Decision>>) {
        // A newer request for the same prompt replaces the old one, which then answers nothing.
        self.lock().insert(approval_id, reply);
    }

    /// Approvals the user can answer here right now.
    pub fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.lock().keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Sends the answer. `false` when the prompt no longer waits here.
    pub fn answer(&self, approval_id: &str, decision: Decision) -> bool {
        match self.lock().remove(approval_id) {
            Some(reply) => reply.send(Some(decision)).is_ok(),
            None => false,
        }
    }

    /// Hands the prompt back to the harness. `false` when it was not waiting.
    pub fn release(&self, approval_id: &str) -> bool {
        self.lock().remove(approval_id).is_some()
    }

    /// Lets go of prompts that are no longer open, e.g. answered in the
    /// terminal, or whose shim gave up. Returns whether anything changed.
    pub fn retain_open(&self, open: &HashSet<String>) -> bool {
        let mut waiting = self.lock();
        let before = waiting.len();
        waiting.retain(|id, reply| open.contains(id) && !reply.is_closed());
        waiting.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_once_and_lets_go_of_closed_prompts() {
        let decisions = Decisions::default();
        let (tx, mut rx) = oneshot::channel();
        decisions.register("s#1".into(), tx);
        assert_eq!(decisions.ids(), ["s#1"]);
        assert!(decisions.answer("s#1", Decision::Allow));
        assert_eq!(rx.try_recv().unwrap(), Some(Decision::Allow));
        assert!(!decisions.answer("s#1", Decision::Allow), "only once");

        let (tx, mut rx) = oneshot::channel::<Option<Decision>>();
        decisions.register("s#2".into(), tx);
        assert!(decisions.release("s#2"));
        assert!(rx.try_recv().is_err(), "released prompts get no answer");

        let (tx, _rx) = oneshot::channel();
        decisions.register("s#3".into(), tx);
        let (tx, rx) = oneshot::channel();
        decisions.register("s#4".into(), tx);
        drop(rx); // the shim gave up
        let open: HashSet<String> = ["s#4".to_owned()].into();
        assert!(decisions.retain_open(&open));
        assert!(decisions.ids().is_empty());
        assert!(!decisions.retain_open(&open));
    }
}
