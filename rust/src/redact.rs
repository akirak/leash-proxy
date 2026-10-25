//! Credential-hygiene helpers for redacting sensitive request data before
//! it is written to audit logs (ADR 0001).
//!
//! Redaction is allowlist-based: paths are reduced to their path component
//! (query strings and fragments dropped) and only explicitly allowlisted
//! headers are ever recorded.

use std::collections::BTreeMap;

use http::HeaderMap;

/// Headers considered safe to record in audit logs by default.
///
/// This is an allowlist: anything not listed here is dropped rather than
/// logged. Credentials such as `authorization` or `cookie` are deliberately
/// absent.
pub const DEFAULT_HEADER_ALLOWLIST: &[&str] = &[
    "host",
    "user-agent",
    "accept",
    "content-type",
    "content-length",
];

const MAX_PATH_LEN: usize = 512;
const MAX_HEADER_VALUE_LEN: usize = 256;

/// Reduce a path-with-query string to a redacted, bounded path.
///
/// Query strings and fragments can carry tokens, so they are stripped
/// entirely rather than partially redacted.
pub fn redact_path(path_and_query: &str) -> String {
    let path = path_and_query.split(['?', '#']).next().unwrap_or("");
    let mut redacted: String = path.chars().take(MAX_PATH_LEN).collect();
    if redacted.is_empty() {
        redacted.push('/');
    }
    redacted
}

/// Filter headers down to the allowlist.
///
/// Returns lowercased header names to redacted (bounded) values. Headers not
/// in the allowlist are dropped; binary header values are replaced with a
/// size marker.
pub fn redact_headers(
    headers: &HeaderMap,
    allowlist: &[impl AsRef<str>],
) -> BTreeMap<String, String> {
    let allowed: Vec<String> = allowlist
        .iter()
        .map(|name| name.as_ref().to_ascii_lowercase())
        .collect();
    let mut out = BTreeMap::new();
    for (name, value) in headers {
        let key = name.as_str().to_ascii_lowercase();
        if !allowed.contains(&key) {
            continue;
        }
        let value = match value.to_str() {
            Ok(text) => text.to_string(),
            Err(_) => format!("<{} binary bytes>", value.len()),
        };
        let value: String = value.chars().take(MAX_HEADER_VALUE_LEN).collect();
        out.insert(key, value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_path_strips_query_and_fragment() {
        assert_eq!(
            redact_path("/repos/octocat/hello?token=sekrit"),
            "/repos/octocat/hello"
        );
        assert_eq!(redact_path("/a/b#c"), "/a/b");
        assert_eq!(redact_path(""), "/");
        assert_eq!(redact_path("/x"), "/x");
    }

    #[test]
    fn redact_path_truncates_very_long_paths() {
        let long = format!("/{}x", "a".repeat(2048));
        assert_eq!(redact_path(&long).len(), MAX_PATH_LEN);
    }

    #[test]
    fn redact_headers_keeps_only_allowlisted_names() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "api.github.com".parse().unwrap());
        headers.insert("user-agent", "agent/1.0".parse().unwrap());
        headers.insert("authorization", "Bearer sekrit".parse().unwrap());
        headers.insert("cookie", "session=sekrit".parse().unwrap());

        let redacted = redact_headers(&headers, &["host", "user-agent"]);
        assert_eq!(
            redacted.get("host").map(String::as_str),
            Some("api.github.com")
        );
        assert_eq!(
            redacted.get("user-agent").map(String::as_str),
            Some("agent/1.0")
        );
        assert!(!redacted.contains_key("authorization"));
        assert!(!redacted.contains_key("cookie"));
    }
}
