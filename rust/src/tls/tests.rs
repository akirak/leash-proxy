use super::*;
use openssl::rsa::Rsa;
use tempfile::TempDir;

pub(crate) fn ca_certificate(
    key: &PKey<Private>,
    is_ca: bool,
    can_sign: bool,
    valid: bool,
) -> X509 {
    let mut cert = X509::builder().unwrap();
    cert.set_version(2).unwrap();
    let serial = BigNum::from_u32(1).unwrap().to_asn1_integer().unwrap();
    cert.set_serial_number(&serial).unwrap();
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "test operator CA").unwrap();
    let name = name.build();
    cert.set_subject_name(&name).unwrap();
    cert.set_issuer_name(&name).unwrap();
    cert.set_pubkey(key).unwrap();
    let before = Asn1Time::from_unix(if valid { 0 } else { 1 }).unwrap();
    let after = if valid {
        Asn1Time::days_from_now(7).unwrap()
    } else {
        Asn1Time::from_unix(2).unwrap()
    };
    cert.set_not_before(&before).unwrap();
    cert.set_not_after(&after).unwrap();
    let mut constraints = BasicConstraints::new();
    constraints.critical();
    if is_ca {
        constraints.ca();
    }
    cert.append_extension(constraints.build().unwrap()).unwrap();
    let mut usage = KeyUsage::new();
    if can_sign {
        usage.key_cert_sign().crl_sign();
    } else {
        usage.digital_signature();
    }
    cert.append_extension(usage.critical().build().unwrap())
        .unwrap();
    cert.sign(key, MessageDigest::sha256()).unwrap();
    cert.build()
}

pub(crate) fn ec_key() -> PKey<Private> {
    PKey::from_ec_key(
        EcKey::generate(&EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap()).unwrap(),
    )
    .unwrap()
}

pub(crate) fn fixture() -> (TempDir, X509) {
    let directory = tempfile::tempdir().unwrap();
    let key = ec_key();
    let cert = ca_certificate(&key, true, true, true);
    write_ca(directory.path(), &cert, &key);
    (directory, cert)
}

fn write_ca(directory: &Path, cert: &X509, key: &PKey<Private>) {
    fs::write(directory.join("ca-cert.pem"), cert.to_pem().unwrap()).unwrap();
    fs::write(
        directory.join("ca-key.pem"),
        key.private_key_to_pem_pkcs8().unwrap(),
    )
    .unwrap();
}

pub(crate) fn trusting(cert: &X509) -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from(cert.to_der().unwrap()))
        .unwrap();
    Arc::new(
        rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth(),
    )
}

#[test]
fn loads_existing_ca_without_modifying_files() {
    let (dir, _) = fixture();
    let before_cert = fs::read(dir.path().join("ca-cert.pem")).unwrap();
    let before_key = fs::read(dir.path().join("ca-key.pem")).unwrap();
    let ca = CertificateAuthority::load(dir.path()).unwrap();
    ca.server_config("api.example.com").unwrap();
    ca.server_config("127.0.0.1").unwrap();
    ca.server_config("::1").unwrap();
    assert_eq!(
        fs::read(dir.path().join("ca-cert.pem")).unwrap(),
        before_cert
    );
    assert_eq!(fs::read(dir.path().join("ca-key.pem")).unwrap(), before_key);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn accepts_combined_mitmproxy_style_pem() {
    let (dir, _) = fixture();
    let mut combined = fs::read(dir.path().join("ca-key.pem")).unwrap();
    combined.extend(fs::read(dir.path().join("ca-cert.pem")).unwrap());
    fs::write(dir.path().join("ca-key.pem"), combined).unwrap();
    CertificateAuthority::load(dir.path())
        .unwrap()
        .server_config("example.com")
        .unwrap();
}

#[cfg(unix)]
#[test]
fn accepts_read_only_ca_directory() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, _) = fixture();
    for file in ["ca-cert.pem", "ca-key.pem"] {
        fs::set_permissions(dir.path().join(file), fs::Permissions::from_mode(0o400)).unwrap();
    }
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let ca = CertificateAuthority::load(dir.path()).unwrap();
    ca.server_config("example.com").unwrap();
    // Restore directory permissions so TempDir can remove its contents.
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn accepts_openssl_rsa_pkcs1_and_pkcs8_keys() {
    let directory = tempfile::tempdir().unwrap();
    let rsa = Rsa::generate(2048).unwrap();
    let pkcs1 = rsa.private_key_to_pem().unwrap();
    let key = PKey::from_rsa(rsa).unwrap();
    let cert = ca_certificate(&key, true, true, true);
    write_ca(directory.path(), &cert, &key);
    CertificateAuthority::load(directory.path())
        .unwrap()
        .server_config("example.com")
        .unwrap();
    fs::write(directory.path().join("ca-key.pem"), pkcs1).unwrap();
    CertificateAuthority::load(directory.path())
        .unwrap()
        .server_config("example.com")
        .unwrap();
}

#[test]
fn accepts_openssl_sec1_ec_key() {
    let (dir, _) = fixture();
    let key = ec_key();
    let cert = ca_certificate(&key, true, true, true);
    write_ca(dir.path(), &cert, &key);
    fs::write(
        dir.path().join("ca-key.pem"),
        key.ec_key().unwrap().private_key_to_pem().unwrap(),
    )
    .unwrap();
    CertificateAuthority::load(dir.path()).unwrap();
}

#[test]
fn rejects_missing_malformed_encrypted_and_mismatched_material() {
    let dir = tempfile::tempdir().unwrap();
    assert!(CertificateAuthority::load(dir.path()).is_err());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    let (dir, _) = fixture();
    fs::write(dir.path().join("ca-cert.pem"), b"not a certificate").unwrap();
    assert!(CertificateAuthority::load(dir.path()).is_err());
    let (dir, _) = fixture();
    fs::write(dir.path().join("ca-key.pem"), b"not a key").unwrap();
    assert!(CertificateAuthority::load(dir.path()).is_err());
    let (dir, _) = fixture();
    let key = ec_key();
    let encrypted = key
        .private_key_to_pem_pkcs8_passphrase(openssl::symm::Cipher::aes_256_cbc(), b"password")
        .unwrap();
    fs::write(dir.path().join("ca-key.pem"), encrypted).unwrap();
    assert!(
        CertificateAuthority::load(dir.path())
            .err()
            .unwrap()
            .to_string()
            .contains("unencrypted")
    );
    fs::write(
        dir.path().join("ca-key.pem"),
        key.private_key_to_pem_pkcs8().unwrap(),
    )
    .unwrap();
    assert!(
        CertificateAuthority::load(dir.path())
            .err()
            .unwrap()
            .to_string()
            .contains("do not match")
    );
}

#[test]
fn rejects_non_ca_expired_or_non_signing_certificates() {
    for (is_ca, can_sign, valid, error) in [
        (false, true, true, "CA:TRUE"),
        (true, false, true, "keyUsage"),
        (true, true, false, "expired"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let key = ec_key();
        write_ca(
            dir.path(),
            &ca_certificate(&key, is_ca, can_sign, valid),
            &key,
        );
        assert!(
            CertificateAuthority::load(dir.path())
                .err()
                .unwrap()
                .to_string()
                .contains(error)
        );
    }
}
