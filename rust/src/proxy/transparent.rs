//! Linux REDIRECT destination recovery and transparent HTTP/HTTPS handling.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use http::header::HOST;
use http::uri::Authority;
use http::{HeaderValue, Method, Request, Uri, Version};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::{LazyConfigAcceptor, TlsConnector, rustls};

use super::{BoxError, ProxyState};
use crate::tls::CertificateAuthority;

#[cfg(test)]
mod tests;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) struct TransparentConfig {
    pub ca: Arc<CertificateAuthority>,
    pub upstream: Arc<rustls::ClientConfig>,
}

#[derive(Clone)]
pub(super) struct InterceptedConnection {
    pub destination: SocketAddr,
    /// TLS SNI (or destination IP when the client omitted SNI).
    pub tls_host: Option<String>,
    /// TLS ALPN selects a fixed protocol; cleartext uses prior-knowledge detection.
    pub http_version: Option<Version>,
}

impl InterceptedConnection {
    /// Bind every request to this connection's destination and TLS identity.
    /// Never allow CONNECT or disagreement between the HTTP authority,
    /// destination port, and TLS SNI to bypass policy checks.
    pub fn prepare_request<B>(&self, req: &mut Request<B>) -> Result<(), &'static str> {
        let scheme = if self.tls_host.is_some() {
            "https"
        } else {
            "http"
        };
        if req.method() == Method::CONNECT || !req.uri().path().starts_with('/') {
            return Err("transparent mode requires an HTTP request path, not CONNECT");
        }
        let host_count = req.headers().get_all(HOST).iter().count();
        if host_count > 1 {
            return Err("multiple Host headers are not allowed");
        }
        let header_authority = req
            .headers()
            .get(HOST)
            .map(|value| {
                value
                    .to_str()
                    .ok()
                    .and_then(|value| value.parse::<Authority>().ok())
                    .ok_or("invalid Host header")
            })
            .transpose()?;
        let authority = if req.version() == Version::HTTP_2 {
            if req.uri().scheme_str() != Some(scheme) {
                return Err("HTTP/2 :scheme does not match intercepted transport");
            }
            let authority = req
                .uri()
                .authority()
                .ok_or("missing HTTP/2 :authority")?
                .clone();
            if header_authority.as_ref().is_some_and(|header| {
                !header.host().eq_ignore_ascii_case(authority.host())
                    || header.port_u16().unwrap_or(self.destination.port())
                        != authority.port_u16().unwrap_or(self.destination.port())
            }) {
                return Err("Host does not match HTTP/2 :authority");
            }
            authority
        } else {
            if req.uri().scheme().is_some() || req.uri().authority().is_some() {
                return Err("transparent HTTP/1 requires origin-form requests");
            }
            header_authority.clone().ok_or("missing Host header")?
        };
        for authority in std::iter::once(&authority).chain(header_authority.iter()) {
            if authority.host().is_empty()
                || authority.as_str().contains('@')
                || (authority.as_str().len() != authority.host().len()
                    && authority.port_u16().is_none())
            {
                return Err("invalid request authority");
            }
        }
        if authority
            .port_u16()
            .is_some_and(|port| port != self.destination.port())
        {
            return Err("authority port does not match intercepted destination");
        }
        let host = authority
            .host()
            .trim_start_matches('[')
            .trim_end_matches(']');
        if self
            .tls_host
            .as_ref()
            .is_some_and(|tls_host| !tls_host.eq_ignore_ascii_case(host))
        {
            return Err("request authority does not match TLS server name");
        }
        // HTTP/2 clients normally send :authority without Host. Materialize
        // Host for audit logging and for forwarding to HTTP/1.1 origins.
        let header =
            HeaderValue::from_str(authority.as_str()).map_err(|_| "invalid request authority")?;
        req.headers_mut().insert(HOST, header);
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.to_string()
        };
        let path = req.uri().path_and_query().ok_or("missing request path")?;
        let uri: Uri = format!("{scheme}://{host}:{}{path}", self.destination.port())
            .parse()
            .map_err(|_| "invalid intercepted request URI")?;
        *req.uri_mut() = uri;
        Ok(())
    }
}

pub(super) async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    destination: SocketAddr,
    state: Arc<ProxyState>,
) -> Result<(), BoxError> {
    let config = state
        .transparent
        .as_ref()
        .ok_or("transparent mode is not configured")?;
    let mut first_byte = [0];
    let n = tokio::time::timeout(HANDSHAKE_TIMEOUT, stream.peek(&mut first_byte)).await??;
    if n == 0 {
        return Ok(());
    }
    // TLS handshake records start with 0x16. Non-TLS traffic is parsed as
    // HTTP and fails closed if malformed (no opaque TCP pass-through).
    if first_byte[0] == 0x16 {
        let start = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            LazyConfigAcceptor::new(rustls::server::Acceptor::default(), stream),
        )
        .await??;
        let host = start
            .client_hello()
            .server_name()
            .map(str::to_string)
            .unwrap_or_else(|| destination.ip().to_string());
        let ca = config.ca.clone();
        let cert_host = host.clone();
        let server = tokio::task::spawn_blocking(move || ca.server_config(&cert_host)).await??;
        let tls = tokio::time::timeout(HANDSHAKE_TIMEOUT, start.into_stream(server)).await??;
        let http_version = Some(negotiated_version(tls.get_ref().1.alpn_protocol()));
        super::server::serve_http(
            tls,
            peer,
            state,
            Some(InterceptedConnection {
                destination,
                tls_host: Some(host),
                http_version,
            }),
        )
        .await
    } else {
        super::server::serve_http(
            stream,
            peer,
            state,
            Some(InterceptedConnection {
                destination,
                tls_host: None,
                http_version: None,
            }),
        )
        .await
    }
}

fn negotiated_version(alpn: Option<&[u8]>) -> Version {
    if alpn == Some(b"h2") {
        Version::HTTP_2
    } else {
        Version::HTTP_11
    }
}

pub(super) trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// Connect only to the kernel-recovered destination, verifying upstream TLS
/// using the same host that was evaluated by policy.
pub(super) async fn connect_upstream(
    connection: &InterceptedConnection,
    host: &str,
    config: &TransparentConfig,
) -> Result<(Box<dyn Io>, Version), BoxError> {
    let stream = TcpStream::connect(connection.destination).await?;
    if connection.tls_host.is_some() {
        let name = ServerName::try_from(host.to_string())?;
        let tls = TlsConnector::from(config.upstream.clone())
            .connect(name, stream)
            .await?;
        let version = negotiated_version(tls.get_ref().1.alpn_protocol());
        Ok((Box::new(tls), version))
    } else {
        // No opportunistic h2c upgrade on the origin connection. HTTP/1.1
        // is compatible with cleartext HTTP/2 clients on the downstream hop.
        Ok((Box::new(stream), Version::HTTP_11))
    }
}

/// Recover the destination before netfilter REDIRECT. Do not fall back to
/// Host/SNI or the local listener address when destination recovery fails.
#[cfg(target_os = "linux")]
pub(super) fn original_destination(stream: &TcpStream) -> Result<SocketAddr, BoxError> {
    use std::mem::{size_of, zeroed};
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV6};
    use std::os::fd::AsRawFd;

    // SAFETY: Both buffers are initialized, correctly sized sockaddr values.
    // getsockopt receives valid pointers and lengths; the fd remains owned by
    // stream. We inspect a buffer only after a successful, full-sized result.
    unsafe {
        let mut addr: libc::sockaddr_in = zeroed();
        let mut len = size_of::<libc::sockaddr_in>() as libc::socklen_t;
        if libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_IP,
            libc::SO_ORIGINAL_DST,
            &mut addr as *mut _ as *mut libc::c_void,
            &mut len,
        ) == 0
            && len as usize == size_of::<libc::sockaddr_in>()
            && addr.sin_family as i32 == libc::AF_INET
        {
            return Ok(SocketAddr::new(
                Ipv4Addr::from(addr.sin_addr.s_addr.to_ne_bytes()).into(),
                u16::from_be(addr.sin_port),
            ));
        }
        let mut addr: libc::sockaddr_in6 = zeroed();
        let mut len = size_of::<libc::sockaddr_in6>() as libc::socklen_t;
        if libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_IPV6,
            libc::IP6T_SO_ORIGINAL_DST,
            &mut addr as *mut _ as *mut libc::c_void,
            &mut len,
        ) == 0
            && len as usize == size_of::<libc::sockaddr_in6>()
            && addr.sin6_family as i32 == libc::AF_INET6
        {
            return Ok(SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::from(addr.sin6_addr.s6_addr),
                u16::from_be(addr.sin6_port),
                addr.sin6_flowinfo,
                addr.sin6_scope_id,
            )));
        }
    }
    Err(std::io::Error::last_os_error().into())
}

#[cfg(not(target_os = "linux"))]
pub(super) fn original_destination(_stream: &TcpStream) -> Result<SocketAddr, BoxError> {
    Err("transparent mode is only supported on Linux".into())
}
