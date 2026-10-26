//! Proxy API surface and connection handling.

pub mod agent;
pub mod body;
pub mod server;
mod transparent;
pub mod types;

pub use agent::{AgentResolver, StaticAgentResolver};
pub use body::{CountedBody, OnFinish, SharedCounter};
pub use server::{BoxError, ForwardingClient, ProxyBody, ProxyState, serve};
pub use types::{ProxyRequest, Scheme, TlsInfo};
