//! AgentScope core: the normalized data model, the reducer that turns
//! [`AgentEvent`]s into a live graph, the SQLite store, and the local ingest
//! server the hook shim talks to. Nothing here depends on Tauri.

pub mod event;
pub mod ingest;
pub mod model;
pub mod redact;
pub mod reducer;
pub mod repo;
pub mod store;
pub mod tail;

pub use agentscope_protocol::{HarnessId, RawHookEvent};
pub use event::{AgentEvent, EventKind, EventSource};
pub use model::*;
pub use reducer::{Changes, Graph, Snapshot};
pub use store::Store;
