//! Agent (micro VM) identification.

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};

/// Resolves the agent identity of an incoming connection.
///
/// Agents run inside micro VMs; the proxy identifies them by their source
/// address so that Cedar policies can target them (`Agent::"<agent id>"`).
pub trait AgentResolver: Send + Sync {
    fn resolve(&self, source: SocketAddr) -> String;
}

/// A static, configuration-driven mapping of source IPs to agent IDs.
///
/// Connections from unmapped addresses are identified as `ip:<address>` so
/// they still fail closed against policies targeting known agents.
#[derive(Debug, Default)]
pub struct StaticAgentResolver {
    map: BTreeMap<IpAddr, String>,
}

impl StaticAgentResolver {
    /// Build a resolver from a mapping of IP strings to agent IDs.
    ///
    /// Entries that are not valid IP addresses are ignored (and should be
    /// reported by config validation).
    pub fn from_entries(entries: &BTreeMap<String, String>) -> Self {
        let map = entries
            .iter()
            .filter_map(|(ip, id)| ip.parse::<IpAddr>().ok().map(|ip| (ip, id.clone())))
            .collect();
        Self { map }
    }
}

impl AgentResolver for StaticAgentResolver {
    fn resolve(&self, source: SocketAddr) -> String {
        self.map
            .get(&source.ip())
            .cloned()
            .unwrap_or_else(|| format!("ip:{}", source.ip()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_mapped_agents() {
        let mut entries = BTreeMap::new();
        entries.insert("10.0.0.5".to_string(), "vm-alpha".to_string());
        let resolver = StaticAgentResolver::from_entries(&entries);
        let source = "10.0.0.5:33333".parse::<SocketAddr>().unwrap();
        assert_eq!(resolver.resolve(source), "vm-alpha");
    }

    #[test]
    fn falls_back_to_source_ip() {
        let resolver = StaticAgentResolver::default();
        let source = "192.168.1.9:33333".parse::<SocketAddr>().unwrap();
        assert_eq!(resolver.resolve(source), "ip:192.168.1.9");
    }

    #[test]
    fn ignores_invalid_entries() {
        let mut entries = BTreeMap::new();
        entries.insert("not-an-ip".to_string(), "vm-bad".to_string());
        entries.insert("10.0.0.6".to_string(), "vm-beta".to_string());
        let resolver = StaticAgentResolver::from_entries(&entries);
        let source = "10.0.0.6:33333".parse::<SocketAddr>().unwrap();
        assert_eq!(resolver.resolve(source), "vm-beta");
    }
}
