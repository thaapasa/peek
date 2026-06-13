//! Build [`CertInfo`] from a UTF-8 source. Splits the input into PEM
//! blocks (via the `pem` crate) and OpenSSH public-key lines, then
//! dispatches each to its decoder. Decode failures don't abort — they
//! land in `parse_errors` so a single malformed block doesn't suppress
//! everything else in a multi-entry file.

use std::time::{SystemTime, UNIX_EPOCH};

use sha1::Sha1;
use sha2::{Digest, Sha256};
use x509_parser::prelude::{
    CertificateRevocationList, FromDer, GeneralName, X509Certificate, X509CertificationRequest,
};
use x509_parser::public_key::{ECPoint, PublicKey};

use crate::info::Extras;
use crate::input::InputSource;
use crate::input::detect::CertFormat;
use crate::types::cert::info::{
    CertEntry, CertInfo, CertificateEntry, CrlEntry, CsrEntry, KeyEntry, KeyType, SshPubKeyEntry,
    UnknownEntry,
};
use crate::types::text::info::TextStats;
use crate::types::text::info_gather::{SIDECAR_TEXT_LIMIT, gather_text_stats};

/// Collect the cert/key Info sidecar. PEM/JWK read the source text
/// (falling back to text stats / binary if it isn't valid UTF-8 — that
/// handles a `.pem` extension misapplied to a DER blob); DER reads the
/// raw bytes and decodes by structure. Capped at [`SIDECAR_TEXT_LIMIT`]:
/// a multi-GB file claiming either format would otherwise pull the whole
/// blob into memory.
pub fn gather_extras(source: &InputSource, fmt: CertFormat, magic_mime: Option<&str>) -> Extras {
    if let Ok(bs) = source.open_byte_source()
        && bs.len() > SIDECAR_TEXT_LIMIT
    {
        return crate::types::binary::info::gather_extras(magic_mime);
    }
    if fmt == CertFormat::Der {
        return match source.read_bytes(crate::input::limits::Budget::Unbounded(
            "gated by SIDECAR_TEXT_LIMIT above",
        )) {
            Ok(der) => Box::new(gather_der(&der)),
            Err(_) => crate::types::binary::info::gather_extras(magic_mime),
        };
    }
    let Some(text_stats) = gather_text_stats(source) else {
        return crate::types::binary::info::gather_extras(magic_mime);
    };
    let Ok(text) = source.read_text(crate::input::limits::Budget::Unbounded(
        "gated by SIDECAR_TEXT_LIMIT above",
    )) else {
        return crate::types::binary::info::gather_extras(magic_mime);
    };
    Box::new(match fmt {
        CertFormat::Jwk => gather_jwk(&text, text_stats),
        _ => gather(&text, text_stats),
    })
}

/// Build [`CertInfo`] for a PEM-or-SSH-pubkey text. The text-stats
/// sidecar is supplied by the caller (already collected in the
/// streaming pass) so this function only handles cert-specific parse.
pub fn gather(text: &str, text_stats: TextStats) -> CertInfo {
    let mut entries: Vec<CertEntry> = Vec::new();
    let mut parse_errors: Vec<String> = Vec::new();

    // Phase 1: PEM blocks. `parse_many` skips text outside `-----BEGIN`
    // / `-----END` fences, so a file with both PEM blocks and SSH lines
    // (rare but legal) lets phase 2 pick up the SSH lines below.
    let pems = pem::parse_many(text.as_bytes()).unwrap_or_default();
    for pem in &pems {
        match classify_pem_block(pem) {
            Ok(entry) => entries.push(entry),
            Err(e) => parse_errors.push(format!("{}: {e}", pem.tag())),
        }
    }

    // Phase 2: SSH public-key lines. Skip every line inside a PEM
    // fence so a `-----BEGIN OPENSSH PRIVATE KEY-----` body doesn't
    // get its base64 picked up as a malformed ssh-rsa line.
    for line in non_pem_lines(text) {
        if let Some(parsed) = try_parse_ssh_pubkey(line) {
            match parsed {
                Ok(entry) => entries.push(entry),
                Err(e) => parse_errors.push(format!("ssh pubkey: {e}")),
            }
        }
    }

    CertInfo {
        text: Some(text_stats),
        source_label: "PEM",
        entries,
        parse_errors,
    }
}

/// Build [`CertInfo`] for a raw DER (binary ASN.1) file. DER carries no
/// label, so the kind is recovered by structure: try X.509 certificate,
/// then CRL, then CSR, then a PKCS#8 / SPKI key, taking the first that
/// decodes. An all-miss lands in a single `Unknown` entry so the user
/// still sees the byte length rather than an empty section.
pub fn gather_der(der: &[u8]) -> CertInfo {
    let (entries, parse_errors) = match classify_der(der) {
        Ok(entry) => (vec![entry], Vec::new()),
        Err(e) => (
            vec![CertEntry::Unknown(UnknownEntry {
                label: "DER".to_string(),
                der_bytes: der.len(),
            })],
            vec![e],
        ),
    };
    CertInfo {
        text: None,
        source_label: "DER",
        entries,
        parse_errors,
    }
}

/// Build [`CertInfo`] for a JWK / JWK Set. The source is JSON text, so it
/// keeps the text-stats Content block (like PEM); the entries section
/// holds one decoded key per JWK. A JSON parse failure lands in
/// `parse_errors` so the Content section still renders.
pub fn gather_jwk(text: &str, text_stats: TextStats) -> CertInfo {
    let (entries, parse_errors) = match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value) => (
            super::jwk::parse(&value)
                .into_iter()
                .map(CertEntry::JsonWebKey)
                .collect(),
            Vec::new(),
        ),
        Err(e) => (Vec::new(), vec![format!("jwk: {e}")]),
    };
    CertInfo {
        text: Some(text_stats),
        source_label: "JWK",
        entries,
        parse_errors,
    }
}

/// Recover a DER blob's kind by trying each structure in turn. Uses a
/// synthetic label (DER has none) matching the PEM tag each decoder
/// expects, so the rendered entry reads the same as its PEM twin.
fn classify_der(der: &[u8]) -> Result<CertEntry, String> {
    if let Ok(c) = parse_certificate("CERTIFICATE", der) {
        return Ok(CertEntry::Certificate(Box::new(c)));
    }
    if let Ok(c) = parse_crl("X509 CRL", der) {
        return Ok(CertEntry::CertificateRevocationList(Box::new(c)));
    }
    if let Ok(c) = parse_csr("CERTIFICATE REQUEST", der) {
        return Ok(CertEntry::CertificateRequest(Box::new(c)));
    }
    // A bare key: PKCS#8 private key first, then a SubjectPublicKeyInfo.
    let (key_type, bits) = classify_pkcs8(der);
    if key_type != KeyType::Other {
        return Ok(CertEntry::PrivateKey(KeyEntry {
            label: "PRIVATE KEY".to_string(),
            key_type,
            key_size_bits: bits,
        }));
    }
    let (key_type, bits) = classify_public_key(der);
    if key_type != KeyType::Other {
        return Ok(CertEntry::PublicKey(KeyEntry {
            label: "PUBLIC KEY".to_string(),
            key_type,
            key_size_bits: bits,
        }));
    }
    Err("not a recognised DER certificate / CRL / CSR / key".to_string())
}

/// Iterator over lines of `text` that fall outside any
/// `-----BEGIN …-----` / `-----END …-----` fence. Captures the
/// payload-or-comment lines used by SSH pubkey detection.
fn non_pem_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("-----BEGIN ") {
            inside = true;
            continue;
        }
        if trimmed.starts_with("-----END ") {
            inside = false;
            continue;
        }
        if !inside {
            out.push(line);
        }
    }
    out
}

/// Dispatch on the PEM label. The label is the source of truth for
/// what's inside — DER-level format depends on it (X.509 cert vs
/// PKCS#10 CSR vs PKCS#1 RSA private key vs PKCS#8 OneAsymmetricKey).
fn classify_pem_block(pem: &pem::Pem) -> Result<CertEntry, String> {
    let tag = pem.tag();
    let der = pem.contents();
    match tag {
        "CERTIFICATE" | "TRUSTED CERTIFICATE" | "X509 CERTIFICATE" => {
            parse_certificate(tag, der).map(|c| CertEntry::Certificate(Box::new(c)))
        }
        "CERTIFICATE REQUEST" | "NEW CERTIFICATE REQUEST" => {
            parse_csr(tag, der).map(|c| CertEntry::CertificateRequest(Box::new(c)))
        }
        "X509 CRL" | "CRL" => {
            parse_crl(tag, der).map(|c| CertEntry::CertificateRevocationList(Box::new(c)))
        }
        "RSA PRIVATE KEY" => Ok(CertEntry::PrivateKey(KeyEntry {
            label: tag.to_string(),
            key_type: KeyType::Rsa,
            key_size_bits: rsa_pkcs1_bits(der),
        })),
        "EC PRIVATE KEY" => Ok(CertEntry::PrivateKey(KeyEntry {
            label: tag.to_string(),
            key_type: KeyType::Ec(ec_curve_from_sec1(der).unwrap_or_default()),
            key_size_bits: ec_curve_from_sec1(der).and_then(ec_bits_for_curve),
        })),
        "DSA PRIVATE KEY" => Ok(CertEntry::PrivateKey(KeyEntry {
            label: tag.to_string(),
            key_type: KeyType::Dsa,
            key_size_bits: None,
        })),
        "PRIVATE KEY" => {
            // PKCS#8 OneAsymmetricKey { version, algorithm, privateKey, … }.
            // The algorithm OID alone is enough to type the key (RSA / EC /
            // Ed25519 / DSA); bit size for RSA comes from re-parsing the
            // wrapped PKCS#1 from the privateKey OCTET STRING.
            let (key_type, bits) = classify_pkcs8(der);
            Ok(CertEntry::PrivateKey(KeyEntry {
                label: tag.to_string(),
                key_type,
                key_size_bits: bits,
            }))
        }
        "ENCRYPTED PRIVATE KEY" | "OPENSSH PRIVATE KEY" => Ok(CertEntry::PrivateKey(KeyEntry {
            label: tag.to_string(),
            key_type: KeyType::Other,
            key_size_bits: None,
        })),
        "PUBLIC KEY" | "RSA PUBLIC KEY" => {
            let (key_type, bits) = classify_public_key(der);
            Ok(CertEntry::PublicKey(KeyEntry {
                label: tag.to_string(),
                key_type,
                key_size_bits: bits,
            }))
        }
        _ => Ok(CertEntry::Unknown(UnknownEntry {
            label: tag.to_string(),
            der_bytes: der.len(),
        })),
    }
}

fn parse_certificate(tag: &str, der: &[u8]) -> Result<CertificateEntry, String> {
    let (_, cert) = X509Certificate::from_der(der).map_err(|e| e.to_string())?;
    let tbs = cert.tbs_certificate;
    let subject = render_name(&tbs.subject.to_string());
    let issuer = render_name(&tbs.issuer.to_string());

    let validity = tbs.validity();
    let not_before = format_iso8601(validity.not_before.timestamp());
    let not_after = format_iso8601(validity.not_after.timestamp());
    let days_remaining = days_until(validity.not_after.timestamp());

    let mut san_dns = Vec::new();
    let mut san_ip = Vec::new();
    let mut san_email = Vec::new();
    let mut san_uri = Vec::new();
    if let Ok(Some(ext)) = tbs.subject_alternative_name() {
        for name in &ext.value.general_names {
            collect_general_name(
                name,
                &mut san_dns,
                &mut san_ip,
                &mut san_email,
                &mut san_uri,
            );
        }
    }

    let (key_algorithm, key_size_bits) = describe_spki(&tbs.subject_pki);
    let signature_algorithm = oid_label(&tbs.signature.algorithm.to_id_string());

    let fingerprint_sha1 = colon_hex(&Sha1::digest(der));
    let fingerprint_sha256 = colon_hex(&Sha256::digest(der));

    let is_ca = tbs
        .basic_constraints()
        .ok()
        .flatten()
        .map(|e| e.value.ca)
        .unwrap_or(false);

    let mut key_usages = Vec::new();
    if let Ok(Some(ku)) = tbs.key_usage() {
        let k = &ku.value;
        if k.digital_signature() {
            key_usages.push("Digital Signature".into());
        }
        if k.non_repudiation() {
            key_usages.push("Non-Repudiation".into());
        }
        if k.key_encipherment() {
            key_usages.push("Key Encipherment".into());
        }
        if k.data_encipherment() {
            key_usages.push("Data Encipherment".into());
        }
        if k.key_agreement() {
            key_usages.push("Key Agreement".into());
        }
        if k.key_cert_sign() {
            key_usages.push("Certificate Sign".into());
        }
        if k.crl_sign() {
            key_usages.push("CRL Sign".into());
        }
        if k.encipher_only() {
            key_usages.push("Encipher Only".into());
        }
        if k.decipher_only() {
            key_usages.push("Decipher Only".into());
        }
    }

    let mut extended_key_usages = Vec::new();
    if let Ok(Some(eku)) = tbs.extended_key_usage() {
        let e = &eku.value;
        if e.server_auth {
            extended_key_usages.push("TLS Server Auth".into());
        }
        if e.client_auth {
            extended_key_usages.push("TLS Client Auth".into());
        }
        if e.code_signing {
            extended_key_usages.push("Code Signing".into());
        }
        if e.email_protection {
            extended_key_usages.push("Email Protection".into());
        }
        if e.time_stamping {
            extended_key_usages.push("Time Stamping".into());
        }
        if e.ocsp_signing {
            extended_key_usages.push("OCSP Signing".into());
        }
        for oid in &e.other {
            extended_key_usages.push(format!("OID {oid}"));
        }
    }

    let self_signed = tbs.subject.as_raw() == tbs.issuer.as_raw();

    Ok(CertificateEntry {
        label: tag.to_string(),
        subject,
        issuer,
        serial_hex: colon_hex(tbs.raw_serial()),
        not_before,
        not_after,
        days_remaining,
        san_dns,
        san_ip,
        san_email,
        san_uri,
        key_algorithm,
        key_size_bits,
        signature_algorithm,
        fingerprint_sha1,
        fingerprint_sha256,
        is_ca,
        self_signed,
        version: tbs.version().0 + 1,
        key_usages,
        extended_key_usages,
    })
}

fn parse_csr(tag: &str, der: &[u8]) -> Result<CsrEntry, String> {
    let (_, csr) = X509CertificationRequest::from_der(der).map_err(|e| e.to_string())?;
    let cri = &csr.certification_request_info;
    let subject = render_name(&cri.subject.to_string());

    let mut san_dns = Vec::new();
    let mut san_ip = Vec::new();
    let mut san_email = Vec::new();
    let mut san_uri = Vec::new();
    if let Some(reqs) = csr.requested_extensions() {
        for parsed in reqs {
            if let x509_parser::extensions::ParsedExtension::SubjectAlternativeName(san) = parsed {
                for name in &san.general_names {
                    collect_general_name(
                        name,
                        &mut san_dns,
                        &mut san_ip,
                        &mut san_email,
                        &mut san_uri,
                    );
                }
            }
        }
    }

    let (key_algorithm, key_size_bits) = describe_spki(&cri.subject_pki);
    let signature_algorithm = oid_label(&csr.signature_algorithm.algorithm.to_id_string());

    Ok(CsrEntry {
        label: tag.to_string(),
        subject,
        san_dns,
        san_ip,
        san_email,
        san_uri,
        key_algorithm,
        key_size_bits,
        signature_algorithm,
    })
}

fn parse_crl(tag: &str, der: &[u8]) -> Result<CrlEntry, String> {
    let (_, crl) = CertificateRevocationList::from_der(der).map_err(|e| e.to_string())?;
    let tbs = &crl.tbs_cert_list;
    let issuer = render_name(&tbs.issuer.to_string());
    let this_update = format_iso8601(tbs.this_update.timestamp());
    let next_update = tbs.next_update.map(|t| format_iso8601(t.timestamp()));
    let revoked_count = crl.iter_revoked_certificates().count();
    let signature_algorithm = oid_label(&tbs.signature.algorithm.to_id_string());

    Ok(CrlEntry {
        label: tag.to_string(),
        issuer,
        this_update,
        next_update,
        revoked_count,
        signature_algorithm,
    })
}

/// Try `line` as an OpenSSH public-key line (`<algo> <base64> [comment]`).
/// `None` means "not an SSH pubkey line, skip"; `Some(Err)` means "looked
/// like one but parse failed" so the error surfaces in `parse_errors`.
fn try_parse_ssh_pubkey(line: &str) -> Option<Result<CertEntry, String>> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let algo = trimmed.split_whitespace().next()?;
    if !is_ssh_pubkey_algo(algo) {
        return None;
    }
    match ssh_key::PublicKey::from_openssh(trimmed) {
        Ok(key) => Some(Ok(CertEntry::SshPublicKey(SshPubKeyEntry {
            algorithm: key.algorithm().as_str().to_string(),
            bits: ssh_pubkey_bits(&key),
            comment: key.comment().to_string(),
            fingerprint_sha256: key.fingerprint(ssh_key::HashAlg::Sha256).to_string(),
        }))),
        Err(e) => Some(Err(e.to_string())),
    }
}

fn is_ssh_pubkey_algo(s: &str) -> bool {
    matches!(
        s,
        "ssh-rsa"
            | "ssh-dss"
            | "ssh-ed25519"
            | "ecdsa-sha2-nistp256"
            | "ecdsa-sha2-nistp384"
            | "ecdsa-sha2-nistp521"
            | "sk-ssh-ed25519@openssh.com"
            | "sk-ecdsa-sha2-nistp256@openssh.com"
    )
}

fn ssh_pubkey_bits(key: &ssh_key::PublicKey) -> Option<usize> {
    // RSA modulus bits come from the public-key wire form (avoids
    // depending on ssh-key's `ecdsa` feature flag to reach the typed
    // EC variants). Everything else maps directly off the algorithm
    // identifier.
    match key.algorithm().as_str() {
        "ssh-rsa" => key.key_data().rsa().map(|rsa| bit_len(rsa.n.as_bytes())),
        "ssh-ed25519" | "sk-ssh-ed25519@openssh.com" => Some(256),
        "ecdsa-sha2-nistp256" | "sk-ecdsa-sha2-nistp256@openssh.com" => Some(256),
        "ecdsa-sha2-nistp384" => Some(384),
        "ecdsa-sha2-nistp521" => Some(521),
        _ => None,
    }
}

/// Bit-length of a big-endian unsigned integer encoded as bytes
/// (leading-zero byte stripped or not). Used to compute RSA modulus
/// bit size for several key formats.
fn bit_len(bytes: &[u8]) -> usize {
    let first_nonzero = bytes.iter().position(|&b| b != 0);
    match first_nonzero {
        Some(i) => (bytes.len() - i) * 8 - bytes[i].leading_zeros() as usize,
        None => 0,
    }
}

/// Render a subject/issuer DN. x509-parser's `Display` for `X509Name`
/// returns the comma-joined RDN list (`CN=foo, O=Bar Inc, …`).
fn render_name(s: &str) -> String {
    if s.is_empty() {
        "(empty)".to_string()
    } else {
        s.to_string()
    }
}

fn describe_spki(spki: &x509_parser::x509::SubjectPublicKeyInfo<'_>) -> (String, Option<usize>) {
    let algo_oid = spki.algorithm.algorithm.to_id_string();
    let label = match algo_oid.as_str() {
        "1.2.840.113549.1.1.1" => "RSA".to_string(),
        "1.2.840.10045.2.1" => match spki.algorithm.parameters.as_ref().and_then(curve_from_any) {
            Some(curve) => format!("EC ({curve})"),
            None => "EC".to_string(),
        },
        "1.3.101.112" => "Ed25519".to_string(),
        "1.3.101.113" => "Ed448".to_string(),
        "1.2.840.10040.4.1" => "DSA".to_string(),
        _ => oid_label(&algo_oid),
    };
    let bits = match spki.parsed() {
        Ok(PublicKey::RSA(rsa)) => Some(rsa.key_size()),
        Ok(PublicKey::EC(ec)) => Some(ec_point_bits(&ec)),
        Ok(PublicKey::DSA(y)) | Ok(PublicKey::GostR3410(y)) => Some(bit_len(y)),
        Ok(PublicKey::GostR3410_2012(b)) => Some(b.len() * 8),
        Ok(PublicKey::Unknown(_)) | Err(_) => match algo_oid.as_str() {
            "1.3.101.112" => Some(256),
            "1.3.101.113" => Some(456),
            _ => None,
        },
    };
    (label, bits)
}

/// EC point uncompressed length → curve bits. Format is `04 || X || Y`
/// for an uncompressed point; X and Y are each `bits/8` bytes.
fn ec_point_bits(ec: &ECPoint<'_>) -> usize {
    let raw_len = ec.data().len();
    let coord_bytes = raw_len.saturating_sub(1) / 2;
    coord_bytes * 8
}

fn curve_from_any(any: &x509_parser::der_parser::asn1_rs::Any<'_>) -> Option<String> {
    let (_, oid) = x509_parser::der_parser::asn1_rs::Oid::from_der(any.as_bytes()).ok()?;
    Some(curve_from_oid(&oid.to_id_string()))
}

fn curve_from_oid(oid: &str) -> String {
    match oid {
        "1.2.840.10045.3.1.7" => "P-256",
        "1.3.132.0.34" => "P-384",
        "1.3.132.0.35" => "P-521",
        "1.3.132.0.10" => "secp256k1",
        "1.3.132.0.33" => "secp224r1",
        "1.3.132.0.32" => "secp224k1",
        _ => return format!("OID {oid}"),
    }
    .to_string()
}

fn ec_bits_for_curve(curve: String) -> Option<usize> {
    match curve.as_str() {
        "P-256" | "secp256k1" => Some(256),
        "P-384" => Some(384),
        "P-521" => Some(521),
        "secp224r1" | "secp224k1" => Some(224),
        _ => None,
    }
}

/// RSA private key in PKCS#1 form is an ASN.1 SEQUENCE whose second
/// element is the modulus n (INTEGER). Hand-crawl the DER far enough
/// to read its tagged length so we can compute the bit size — full
/// PKCS#1 parsing would pull in another crate without paying for
/// itself in the info section.
fn rsa_pkcs1_bits(der: &[u8]) -> Option<usize> {
    let (inner, _) = read_tlv(der, 0x30)?;
    // skip version (INTEGER)
    let (_, after_ver) = read_tlv(inner, 0x02)?;
    let (modulus, _) = read_tlv(after_ver, 0x02)?;
    Some(bit_len(modulus))
}

/// SEC1 EC private key: SEQUENCE { version, privateKey OCTET STRING,
/// [0] ECParameters … }. The named curve OID is inside the explicit
/// [0] tag (`0xA0`). We peek for the OID and look it up.
fn ec_curve_from_sec1(der: &[u8]) -> Option<String> {
    let (inner, _) = read_tlv(der, 0x30)?;
    let (_, after_ver) = read_tlv(inner, 0x02)?;
    let (_, after_pk) = read_tlv(after_ver, 0x04)?;
    let (params_body, _) = read_tlv(after_pk, 0xA0)?;
    let (oid_body, _) = read_tlv(params_body, 0x06)?;
    let oid = oid_from_der(oid_body)?;
    Some(curve_from_oid(&oid))
}

/// PKCS#8 OneAsymmetricKey: SEQUENCE { version, AlgorithmIdentifier,
/// OCTET STRING privateKey, … }. We read the algorithm OID to decide
/// the key type; for RSA we recurse into the privateKey octets (it's
/// a PKCS#1 RSAPrivateKey) to recover the modulus bits.
fn classify_pkcs8(der: &[u8]) -> (KeyType, Option<usize>) {
    let Some((inner, _)) = read_tlv(der, 0x30) else {
        return (KeyType::Other, None);
    };
    let Some((_, after_ver)) = read_tlv(inner, 0x02) else {
        return (KeyType::Other, None);
    };
    let Some((algo_body, after_algo)) = read_tlv(after_ver, 0x30) else {
        return (KeyType::Other, None);
    };
    let Some((oid_body, params_rest)) = read_tlv(algo_body, 0x06) else {
        return (KeyType::Other, None);
    };
    let Some(oid) = oid_from_der(oid_body) else {
        return (KeyType::Other, None);
    };
    match oid.as_str() {
        "1.2.840.113549.1.1.1" => {
            // RSA. privateKey is OCTET STRING wrapping PKCS#1 RSAPrivateKey.
            let bits = read_tlv(after_algo, 0x04).and_then(|(pk, _)| rsa_pkcs1_bits(pk));
            (KeyType::Rsa, bits)
        }
        "1.2.840.10045.2.1" => {
            let curve = read_tlv(params_rest, 0x06)
                .and_then(|(oid_body, _)| oid_from_der(oid_body))
                .map(|s| curve_from_oid(&s))
                .unwrap_or_default();
            let bits = if curve.is_empty() {
                None
            } else {
                ec_bits_for_curve(curve.clone())
            };
            (KeyType::Ec(curve), bits)
        }
        "1.3.101.112" => (KeyType::Ed25519, Some(256)),
        "1.3.101.113" => (KeyType::Other, Some(456)),
        "1.2.840.10040.4.1" => (KeyType::Dsa, None),
        _ => (KeyType::Other, None),
    }
}

/// SubjectPublicKeyInfo for a bare public key. Same shape as in a
/// certificate, just without the surrounding TbsCertificate.
fn classify_public_key(der: &[u8]) -> (KeyType, Option<usize>) {
    let Some((inner, _)) = read_tlv(der, 0x30) else {
        // RSA PUBLIC KEY (PKCS#1 form) is bare RSAPublicKey { n, e }.
        if let Some(bits) = rsa_pkcs1_bits(der) {
            return (KeyType::Rsa, Some(bits));
        }
        return (KeyType::Other, None);
    };
    let Some((algo_body, after_algo)) = read_tlv(inner, 0x30) else {
        // No AlgorithmIdentifier — assume PKCS#1 RSAPublicKey form.
        if let Some(bits) = rsa_pkcs1_bits(der) {
            return (KeyType::Rsa, Some(bits));
        }
        return (KeyType::Other, None);
    };
    let Some((oid_body, params_rest)) = read_tlv(algo_body, 0x06) else {
        return (KeyType::Other, None);
    };
    let Some(oid) = oid_from_der(oid_body) else {
        return (KeyType::Other, None);
    };
    match oid.as_str() {
        "1.2.840.113549.1.1.1" => {
            // BIT STRING with leading-zero unused-bits byte wraps the
            // PKCS#1 RSAPublicKey body.
            let bits = read_tlv(after_algo, 0x03)
                .and_then(|(bs, _)| bs.split_first())
                .and_then(|(_, body)| rsa_pkcs1_bits(body));
            (KeyType::Rsa, bits)
        }
        "1.2.840.10045.2.1" => {
            let curve = read_tlv(params_rest, 0x06)
                .and_then(|(oid_body, _)| oid_from_der(oid_body))
                .map(|s| curve_from_oid(&s))
                .unwrap_or_default();
            let bits = if curve.is_empty() {
                None
            } else {
                ec_bits_for_curve(curve.clone())
            };
            (KeyType::Ec(curve), bits)
        }
        "1.3.101.112" => (KeyType::Ed25519, Some(256)),
        "1.3.101.113" => (KeyType::Other, Some(456)),
        "1.2.840.10040.4.1" => (KeyType::Dsa, None),
        _ => (KeyType::Other, None),
    }
}

/// Read one TLV (tag + length + body) from `der`. Returns the body
/// and the remainder after this TLV. `None` on truncation or
/// tag-mismatch. Supports short-form and long-form length encodings.
fn read_tlv(der: &[u8], expected_tag: u8) -> Option<(&[u8], &[u8])> {
    let (&tag, rest) = der.split_first()?;
    if tag != expected_tag {
        return None;
    }
    let (&first_len, rest) = rest.split_first()?;
    let (len, rest) = if first_len & 0x80 == 0 {
        (first_len as usize, rest)
    } else {
        let n = (first_len & 0x7F) as usize;
        if n == 0 || n > 4 || rest.len() < n {
            return None;
        }
        let (len_bytes, rest) = rest.split_at(n);
        let mut len = 0usize;
        for &b in len_bytes {
            len = (len << 8) | b as usize;
        }
        (len, rest)
    };
    if rest.len() < len {
        return None;
    }
    Some(rest.split_at(len))
}

/// Decode the body of a DER OID (tag stripped) into dotted-decimal
/// form. Standard base-128 SID encoding with the first two arcs
/// folded into one byte (`40*a + b`).
fn oid_from_der(body: &[u8]) -> Option<String> {
    if body.is_empty() {
        return None;
    }
    let first = body[0];
    let arc1 = (first / 40).min(2) as u32;
    let arc2 = (first - 40 * arc1 as u8) as u32;
    let mut out = format!("{arc1}.{arc2}");
    let mut acc: u32 = 0;
    for &b in &body[1..] {
        acc = (acc << 7) | (b & 0x7F) as u32;
        if b & 0x80 == 0 {
            out.push('.');
            out.push_str(&acc.to_string());
            acc = 0;
        }
    }
    Some(out)
}

fn collect_general_name<'a>(
    name: &GeneralName<'a>,
    dns: &mut Vec<String>,
    ip: &mut Vec<String>,
    email: &mut Vec<String>,
    uri: &mut Vec<String>,
) {
    match name {
        GeneralName::DNSName(s) => dns.push((*s).to_string()),
        GeneralName::RFC822Name(s) => email.push((*s).to_string()),
        GeneralName::URI(s) => uri.push((*s).to_string()),
        GeneralName::IPAddress(b) => ip.push(format_ip(b)),
        _ => {}
    }
}

fn format_ip(b: &[u8]) -> String {
    match b.len() {
        4 => format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]),
        16 => {
            let groups: Vec<String> = b
                .chunks(2)
                .map(|c| format!("{:x}", u16::from_be_bytes([c[0], c[1]])))
                .collect();
            groups.join(":")
        }
        _ => colon_hex(b),
    }
}

fn colon_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            out.push(':');
        }
        out.push_str(&format!("{:02X}", b));
    }
    out
}

/// Map a known signature OID to a short name; fall back to the dotted
/// form so the user always sees something interpretable.
fn oid_label(oid: &str) -> String {
    match oid {
        "1.2.840.113549.1.1.5" => "sha1WithRSAEncryption".into(),
        "1.2.840.113549.1.1.11" => "sha256WithRSAEncryption".into(),
        "1.2.840.113549.1.1.12" => "sha384WithRSAEncryption".into(),
        "1.2.840.113549.1.1.13" => "sha512WithRSAEncryption".into(),
        "1.2.840.113549.1.1.10" => "rsassaPss".into(),
        "1.2.840.10045.4.1" => "ecdsa-with-SHA1".into(),
        "1.2.840.10045.4.3.2" => "ecdsa-with-SHA256".into(),
        "1.2.840.10045.4.3.3" => "ecdsa-with-SHA384".into(),
        "1.2.840.10045.4.3.4" => "ecdsa-with-SHA512".into(),
        "1.3.101.112" => "Ed25519".into(),
        "1.3.101.113" => "Ed448".into(),
        "1.2.840.10040.4.3" => "dsa-with-SHA1".into(),
        "1.2.840.113549.1.1.1" => "rsaEncryption".into(),
        _ => format!("OID {oid}"),
    }
}

fn format_iso8601(unix_secs: i64) -> String {
    // x509-parser exposes ASN1Time::timestamp() as seconds since epoch.
    // A tiny Gregorian conversion keeps us off chrono/time deps; the
    // info row is purely informational so an extra dependency would be
    // weight without obvious win.
    let (y, mo, d, h, mi, s) = gregorian(unix_secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn days_until(unix_secs: i64) -> Option<i64> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    let diff = unix_secs - now;
    Some(diff.div_euclid(86_400))
}

/// Convert a Unix timestamp to UTC Y/M/D/h/m/s components.
/// Howard Hinnant's `days_from_civil` inverse, integer-only.
fn gregorian(unix_secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = unix_secs.div_euclid(86_400);
    let secs = unix_secs.rem_euclid(86_400);
    let h = (secs / 3600) as u32;
    let mi = ((secs % 3600) / 60) as u32;
    let s = (secs % 60) as u32;
    // Days since 0000-03-01.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = y + if m <= 2 { 1 } else { 0 };
    (y, m, d, h, mi, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn der_fixture() -> Vec<u8> {
        let path = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("test-data/cert-rsa.der");
        std::fs::read(path).expect("cert-rsa.der fixture")
    }

    fn jwks_fixture() -> String {
        let path = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("test-data/keys.jwks");
        std::fs::read_to_string(path).expect("keys.jwks fixture")
    }

    fn entry_kind(e: &crate::types::cert::info::CertEntry) -> &'static str {
        use crate::types::cert::info::CertEntry::*;
        match e {
            Certificate(_) => "Certificate",
            CertificateRequest(_) => "CSR",
            CertificateRevocationList(_) => "CRL",
            PrivateKey(_) => "PrivateKey",
            PublicKey(_) => "PublicKey",
            SshPublicKey(_) => "SshPublicKey",
            JsonWebKey(_) => "JsonWebKey",
            Unknown(_) => "Unknown",
        }
    }

    fn stub_stats() -> crate::types::text::info::TextStats {
        use crate::types::text::info::{Encoding, LineEndings, TextStats};
        TextStats {
            line_count: 0,
            word_count: 0,
            char_count: 0,
            blank_lines: 0,
            longest_line_chars: 0,
            line_endings: LineEndings::Lf,
            indent_style: None,
            encoding: Encoding::Utf8,
            shebang: None,
        }
    }

    #[test]
    fn gather_der_decodes_certificate() {
        use crate::types::cert::info::CertEntry;
        let info = gather_der(&der_fixture());
        assert!(info.text.is_none());
        assert_eq!(info.source_label, "DER");
        assert_eq!(info.entries.len(), 1);
        assert!(info.parse_errors.is_empty());
        match &info.entries[0] {
            CertEntry::Certificate(c) => assert!(!c.subject.is_empty()),
            other => panic!("expected Certificate, got {:?}", entry_kind(other)),
        }
    }

    /// Decode the two-key fixture. The expected RFC 7638 thumbprints are
    /// computed independently (an OpenSSL + Python pass over the same
    /// keys), so this cross-checks peek's thumbprint against a reference
    /// implementation rather than against itself.
    #[test]
    fn jwk_decodes_set_with_thumbprints() {
        use crate::types::cert::info::CertEntry;
        let info = gather_jwk(&jwks_fixture(), stub_stats());
        assert_eq!(info.source_label, "JWK");
        assert!(info.parse_errors.is_empty());
        assert_eq!(info.entries.len(), 2);

        let CertEntry::JsonWebKey(rsa) = &info.entries[0] else {
            panic!("expected JWK entry");
        };
        assert_eq!(rsa.kty, "RSA");
        assert_eq!(rsa.key_size_bits, Some(2048));
        assert_eq!(rsa.alg.as_deref(), Some("RS256"));
        assert_eq!(
            rsa.thumbprint.as_deref(),
            Some("SHA-256:Gc2sKhlHnzx8V3y_pGBuw7mVsXCRsljLz8QRfPok7Q4")
        );

        let CertEntry::JsonWebKey(ec) = &info.entries[1] else {
            panic!("expected JWK entry");
        };
        assert_eq!(ec.kty, "EC");
        assert_eq!(ec.crv.as_deref(), Some("P-256"));
        assert_eq!(ec.key_size_bits, Some(256));
        assert_eq!(ec.key_ops, vec!["verify".to_string()]);
        assert_eq!(
            ec.thumbprint.as_deref(),
            Some("SHA-256:mopLKn99TN2jE4dvMxFAup-xoynNh7THvm-mFf4TQtQ")
        );
    }

    #[test]
    fn read_tlv_short_form() {
        // SEQUENCE (0x30), length 3, body 01 02 03.
        let der = [0x30, 0x03, 0x01, 0x02, 0x03, 0xAA];
        let (body, rest) = read_tlv(&der, 0x30).unwrap();
        assert_eq!(body, &[0x01, 0x02, 0x03]);
        assert_eq!(rest, &[0xAA]);
    }

    #[test]
    fn read_tlv_long_form_length() {
        // Length 0x81 0x80 = 128 bytes.
        let mut der = vec![0x04, 0x81, 0x80];
        der.extend(std::iter::repeat_n(0u8, 128));
        let (body, rest) = read_tlv(&der, 0x04).unwrap();
        assert_eq!(body.len(), 128);
        assert!(rest.is_empty());
    }

    #[test]
    fn read_tlv_tag_mismatch() {
        let der = [0x30, 0x01, 0xAA];
        assert!(read_tlv(&der, 0x02).is_none());
    }

    #[test]
    fn oid_decoder_rsa() {
        // 1.2.840.113549.1.1.1 = rsaEncryption.
        // DER body: 2A 86 48 86 F7 0D 01 01 01.
        let body = [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];
        assert_eq!(oid_from_der(&body).as_deref(), Some("1.2.840.113549.1.1.1"));
    }

    #[test]
    fn oid_decoder_ec_p256() {
        // 1.2.840.10045.3.1.7 = prime256v1.
        let body = [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
        assert_eq!(oid_from_der(&body).as_deref(), Some("1.2.840.10045.3.1.7"));
    }

    #[test]
    fn bit_len_strips_leading_zeros() {
        assert_eq!(bit_len(&[0x80]), 8);
        assert_eq!(bit_len(&[0x01]), 1);
        assert_eq!(bit_len(&[0x00, 0xFF]), 8);
        assert_eq!(bit_len(&[0xFF, 0xFF]), 16);
        assert_eq!(bit_len(&[]), 0);
    }

    #[test]
    fn colon_hex_format() {
        assert_eq!(colon_hex(&[0xAB, 0xCD, 0x01]), "AB:CD:01");
        assert_eq!(colon_hex(&[]), "");
    }

    #[test]
    fn gregorian_epoch() {
        assert_eq!(gregorian(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(gregorian(86_399), (1970, 1, 1, 23, 59, 59));
        // 2026-01-15T12:34:56Z = 1768480496.
        assert_eq!(gregorian(1_768_480_496), (2026, 1, 15, 12, 34, 56));
        // Leap-year boundary: 2024-02-29T00:00:00Z = 1709164800.
        assert_eq!(gregorian(1_709_164_800), (2024, 2, 29, 0, 0, 0));
    }

    #[test]
    fn ssh_pubkey_skip_blank_and_comment() {
        assert!(try_parse_ssh_pubkey("").is_none());
        assert!(try_parse_ssh_pubkey("   ").is_none());
        assert!(try_parse_ssh_pubkey("# a comment").is_none());
        assert!(try_parse_ssh_pubkey("random text").is_none());
    }
}
