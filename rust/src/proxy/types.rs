//! Core proxy API surface: normalized request/response types shared by the
//! connection handlers, policy engines, and audit logging.

use std::fmt;
use std::net::SocketAddr;

use http::{HeaderMap, Method, Version};

/// Request scheme as seen by the proxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// Plain HTTP, forwarded in cleartext.
    Http,
    /// HTTPS received after transparent TLS termination. Absolute-form
    /// `https://` requests on the forward proxy port are rejected.
    Https,
    /// A CONNECT tunnel (`CONNECT host:port`).
    Tunnel,
}

impl Scheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
            Scheme::Tunnel => "tunnel",
        }
    }
}

impl fmt::Display for Scheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// TLS metadata for an audit record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsInfo {
    /// No TLS: the request was plaintext HTTP.
    None,
    /// The connection is a CONNECT tunnel; the proxy did not terminate TLS.
    Tunnel,
    /// The proxy terminated TLS in transparent interception mode.
    Terminated,
}

impl TlsInfo {
    pub fn as_str(self) -> &'static str {
        match self {
            TlsInfo::None => "none",
            TlsInfo::Tunnel => "tunnel",
            TlsInfo::Terminated => "terminated",
        }
    }
}

/// A request as observed by the proxy, normalized for policy evaluation and
/// audit logging.
#[derive(Debug, Clone)]
pub struct ProxyRequest {
    /// Unique identifier assigned by the proxy.
    pub request_id: String,
    /// Identity of the agent (micro VM) the traffic originates from.
    pub agent_id: String,
    /// Source socket address of the agent connection.
    pub source: SocketAddr,
    /// HTTP method, e.g. `GET` or `CONNECT`.
    pub method: Method,
    /// Request scheme (`http`, `https`, or `tunnel` for CONNECT).
    pub scheme: Scheme,
    /// Destination host (without port).
    pub host: String,
    /// Destination port.
    pub port: u16,
    /// Path and query for forwarded requests; empty for CONNECT.
    pub path: String,
    /// HTTP version of the client request.
    pub version: Version,
    /// Original request headers (unredacted; use [`crate::redact`] before
    /// logging).
    pub headers: HeaderMap,
}

impl ProxyRequest {
    /// `host:port` authority of the destination.
    pub fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
