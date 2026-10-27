//! Glue between the ingest server, the adapters, the reducer, and the store.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use agentscope_adapters::Adapter;
use agentscope_core::store::{Duration, StoreError};
use agentscope_core::{Changes, Graph, HarnessId, RawHookEvent, Snapshot, Store};

/// Default history retention (spec section 8).
pub const RETENTION_DAYS: i64 = 7;

pub struct Engine {
    adapters: HashMap<HarnessId, Box<dyn Adapter>>,
    inner: Mutex<Inner>,
}

struct Inner {
    graph: Graph,
    store: Store,
}

impl Engine {
    pub fn open(db_path: &Path) -> Result<Self, StoreError> {
        let mut store = Store::open(db_path)?;
        let pruned = store.prune(Duration::days(RETENTION_DAYS))?;
        tracing::info!(pruned, "store opened");
        let graph = store.load_graph()?;
        Ok(Self {
            adapters: agentscope_adapters::all()
                .into_iter()
                .map(|a| (a.id(), a))
                .collect(),
            inner: Mutex::new(Inner { graph, store }),
        })
    }

    pub fn snapshot(&self) -> Snapshot {
        self.lock().graph.snapshot()
    }

    /// Normalizes, applies, and persists one hook event. Returns whether the graph changed.
    pub fn ingest(&self, raw: &RawHookEvent) -> bool {
        let Some(adapter) = self.adapters.get(&raw.harness) else {
            return false;
        };
        let events = adapter.normalize_hook(raw);
        if events.is_empty() {
            tracing::debug!(harness = %raw.harness, event = %raw.event, "hook event not handled yet");
            return false;
        }
        let mut inner = self.lock();
        let mut changes = Changes::default();
        for ev in &events {
            changes.extend(inner.graph.apply(ev));
            if let Err(err) = inner.store.append_event(ev) {
                tracing::error!(%err, "failed to store event");
            }
        }
        let Inner { graph, store } = &mut *inner;
        if let Err(err) = store.save(graph, &changes) {
            tracing::error!(%err, "failed to save graph");
        }
        !changes.is_empty()
    }

    pub fn detections(&self) -> Vec<agentscope_adapters::Detection> {
        HarnessId::ALL
            .iter()
            .filter_map(|id| self.adapters.get(id))
            .map(|a| a.detect())
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A panic while holding the lock leaves the graph usable; keep serving it.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}
