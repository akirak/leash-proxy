//! Load an operator-managed CA and issue short-lived interception certificates.
//! No CA certificate or CA private key is generated, modified, or written to disk.

use std::fs;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;

use openssl::asn1::Asn1Time;
use openssl::bn::{BigNum, MsbOption};
use openssl::ec::{EcGroup, EcKey};
use openssl::hash::MessageDigest;
use openssl::nid::Nid;
use openssl::pkey::{Id, PKey, Private};
use openssl::x509::extension::{
    BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName,
};
use openssl::x509::{X509, X509NameBuilder};
use tokio_rustls::rustls;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use x509_parser::prelude::*;

use crate::proxy::BoxError;

#[cfg(test)]
pub(crate) mod tests;

pub struct CertificateAuthority {
    chain: Vec<X509>,
    key: PKey<Private>,
}

impl CertificateAuthority {
    /// Read `ca-cert.pem` (signing certificate first, optional chain after it)
    /// and `ca-key.pem` (unencrypted PKCS#1, PKCS#8, or SEC1 PEM).
    pub fn load(directory: &Path) -> Result<Self, BoxError> {
        let cert_path = directory.join("ca-cert.pem");
        let key_path = directory.join("ca-key.pem");
        let cert_pem = fs::read(&cert_path)
            .map_err(|err| format!("failed to read {}: {err}", cert_path.display()))?;
        let key_pem = fs::read(&key_path)
            .map_err(|err| format!("failed to read {}: {err}", key_path.display()))?;
        let chain = X509::stack_from_pem(&cert_pem)
            .map_err(|_| format!("invalid PEM certificate in {}", cert_path.display()))?;
        let cert = chain
            .first()
            .ok_or("ca-cert.pem contains no certificates")?;
        // Supplying a password callback prevents OpenSSL from prompting on stdin.
        let key = PKey::private_key_from_pem_callback(&key_pem, |_| {
            Err(openssl::error::ErrorStack::get())
        })
        .map_err(|_| {
            format!(
                "{} must contain an unencrypted PEM private key",
                key_path.display()
            )
        })?;
        if !cert.public_key()?.public_eq(&key) {
            return Err("CA certificate and private key do not match".into());
        }
        if !matches!(key.id(), Id::RSA | Id::EC) {
            return Err("CA private key must be RSA or EC".into());
        }
        let der = cert.to_der()?;
        let (_, parsed) =
            parse_x509_certificate(&der).map_err(|_| "invalid CA X.509 certificate")?;
        if !parsed.validity().is_valid() {
            return Err("CA certificate is expired or not yet valid".into());
        }
        if !parsed.basic_constraints()?.is_some_and(|ext| ext.value.ca) {
            return Err("CA certificate must have basicConstraints CA:TRUE".into());
        }
        if parsed
            .key_usage()?
            .is_some_and(|ext| !ext.value.key_cert_sign())
        {
            return Err("CA certificate keyUsage does not permit certificate signing".into());
        }
        Ok(Self { chain, key })
    }

    /// Issue a DNS or IP SAN certificate, valid for at most one day and never
    /// outside the supplied CA's validity. Leaf keys stay in memory only.
    pub fn server_config(&self, host: &str) -> Result<Arc<rustls::ServerConfig>, BoxError> {
        // Validate before passing the name to OpenSSL's extension builder.
        rustls::pki_types::ServerName::try_from(host.to_string())?;
        let ca = &self.chain[0];
        let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1)?;
        let key = PKey::from_ec_key(EcKey::generate(&group)?)?;
        let mut builder = X509::builder()?;
        builder.set_version(2)?;
        let mut serial = BigNum::new()?;
        serial.rand(128, MsbOption::MAYBE_ZERO, false)?;
        let serial = serial.to_asn1_integer()?;
        builder.set_serial_number(&serial)?;
        let mut name = X509NameBuilder::new()?;
        name.append_entry_by_nid(Nid::COMMONNAME, "leash-proxy interception")?;
        builder.set_subject_name(&name.build())?;
        builder.set_issuer_name(ca.subject_name())?;
        builder.set_pubkey(&key)?;
        let now = Asn1Time::days_from_now(0)?;
        if now < *ca.not_before() || now >= *ca.not_after() {
            return Err("CA certificate is expired or not yet valid".into());
        }
        let tomorrow = Asn1Time::days_from_now(1)?;
        builder.set_not_before(&now)?;
        builder.set_not_after(if tomorrow < *ca.not_after() {
            &tomorrow
        } else {
            ca.not_after()
        })?;
        builder.append_extension(BasicConstraints::new().critical().build()?)?;
        builder.append_extension(KeyUsage::new().critical().digital_signature().build()?)?;
        builder.append_extension(ExtendedKeyUsage::new().server_auth().build()?)?;
        let mut san = SubjectAlternativeName::new();
        if host.parse::<IpAddr>().is_ok() {
            san.ip(host);
        } else {
            san.dns(host);
        }
        let san = san.build(&builder.x509v3_context(Some(ca), None))?;
        builder.append_extension(san)?;
        builder.sign(&self.key, MessageDigest::sha256())?;
        let mut chain = vec![CertificateDer::from(builder.build().to_der()?)];
        for cert in &self.chain {
            chain.push(CertificateDer::from(cert.to_der()?));
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(
                chain,
                PrivatePkcs8KeyDer::from(key.private_key_to_pkcs8()?).into(),
            )?;
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }
}

/// Verify upstream certificates against the OS trust store, not the MITM CA.
pub fn upstream_config() -> Result<Arc<rustls::ClientConfig>, BoxError> {
    let native = rustls_native_certs::load_native_certs();
    let mut roots = rustls::RootCertStore::empty();
    for cert in native.certs {
        roots.add(cert)?;
    }
    if roots.is_empty() {
        return Err("no upstream TLS trust roots found in the OS trust store".into());
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}
