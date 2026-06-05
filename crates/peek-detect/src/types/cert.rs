//! Cert / key container format + detection.
//!
//! PEM covers `-----BEGIN …-----` blocks (any label) and bare OpenSSH
//! public-key text (single-line `ssh-rsa AAAA…` / `ecdsa-sha2-… AAAA…` /
//! `ssh-ed25519 AAAA…`). DER is the raw ASN.1 binary form an X.509
//! certificate carries without the base64 armour — decoded through the
//! same path, just minus the PEM unwrap. PKCS#12 is tracked in
//! `docs/planned.md` and not yet wired.

use x509_parser::prelude::{FromDer, X509Certificate};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertFormat {
    /// PEM container (or OpenSSH public-key text). Decode is best
    /// effort per block — unknown labels surface as `Unknown` entries.
    Pem,
    /// Raw DER (binary ASN.1) — a single X.509 certificate / CRL / CSR /
    /// key with no base64 armour and no label. The decoder tries each
    /// structure in turn to recover the kind.
    Der,
    /// JSON Web Key or Key Set (`.jwk` / `.jwks`) — a JSON document. Gets
    /// the pretty-printed JSON source view plus a decoded key sidecar.
    Jwk,
}

/// Map a lowercased filename extension to the cert/key container
/// format. Text (PEM) extensions route to `Pem`; the unambiguous binary
/// `.der` routes to `Der`. `.crt` / `.cer` are deliberately left out —
/// they routinely carry *either* encoding, so they resolve by content:
/// a PEM header via [`sniff_pem`], raw DER via [`sniff_der`].
pub fn format_from_ext(ext: &str) -> Option<CertFormat> {
    match ext {
        "pem" | "csr" | "crl" | "key" | "p7b" | "p7c" | "pub" => Some(CertFormat::Pem),
        "der" => Some(CertFormat::Der),
        "jwk" | "jwks" => Some(CertFormat::Jwk),
        _ => None,
    }
}

/// True when a parsed JSON value is a JWK or JWK Set. Lets the content
/// sniffer route a `.json` JWK to the key viewer instead of the generic
/// JSON pretty-printer. Tight (`kty` must be a known type) so unrelated
/// JSON isn't grabbed; the pretty-JSON source view is preserved either
/// way, so nothing is lost on a match.
pub fn sniff_jwk(value: &serde_json::Value) -> bool {
    looks_like_jwk(value)
}

/// True when `value` looks like a JWK (object with a recognised `kty`) or
/// a JWK Set (a `keys` array of such objects). Tight enough that an
/// unrelated JSON file carrying a `kty` or `keys` field won't match.
///
/// The full JWK decode (thumbprints, key sizes) lives in the cert
/// reader's `jwk` module; detection only needs this recognition check.
fn looks_like_jwk(value: &serde_json::Value) -> bool {
    if let Some(keys) = value.get("keys").and_then(serde_json::Value::as_array) {
        return !keys.is_empty() && keys.iter().all(has_known_kty);
    }
    has_known_kty(value)
}

fn has_known_kty(value: &serde_json::Value) -> bool {
    matches!(
        value.get("kty").and_then(serde_json::Value::as_str),
        Some("RSA" | "EC" | "oct" | "OKP")
    )
}

/// True if `text` opens with a recognisable PEM header (after
/// leading whitespace) or an OpenSSH public-key prefix. Used by the
/// text content sniffer when the source has no name to classify by.
pub fn sniff_pem(text: &str) -> bool {
    let trimmed = text.trim_start();
    if trimmed.starts_with("-----BEGIN ") {
        return true;
    }
    SSH_PUBKEY_PREFIXES
        .iter()
        .any(|p| trimmed.starts_with(p) && trimmed.len() > p.len())
}

/// True when `head` is a DER X.509 certificate: a top-level `SEQUENCE`
/// with a 2-byte length (`0x30 0x82`, the universal shape for any
/// real-world cert) that actually decodes. The full parse — not just the
/// magic bytes — keeps unrelated ASN.1 / BER blobs from being mislabelled
/// as certs, so this is safe to run on any unnamed binary head.
pub fn sniff_der(head: &[u8]) -> bool {
    head.starts_with(&[0x30, 0x82]) && X509Certificate::from_der(head).is_ok()
}

/// OpenSSH public-key algorithm prefixes that appear at the start of a
/// `.pub` line. Matches the canonical algorithm IDs from RFC 4253 and
/// `ssh-keygen(1)`; covers RSA / DSA / ECDSA / Ed25519 plus the
/// security-key (FIDO/U2F) variants.
const SSH_PUBKEY_PREFIXES: &[&str] = &[
    "ssh-rsa ",
    "ssh-dss ",
    "ssh-ed25519 ",
    "ecdsa-sha2-nistp256 ",
    "ecdsa-sha2-nistp384 ",
    "ecdsa-sha2-nistp521 ",
    "sk-ssh-ed25519@openssh.com ",
    "sk-ecdsa-sha2-nistp256@openssh.com ",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn der_fixture() -> Vec<u8> {
        // Fixtures live in the workspace-root `test-data/`; this crate's
        // manifest dir is `crates/peek-detect`.
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-data/cert-rsa.der");
        std::fs::read(path).expect("cert-rsa.der fixture")
    }

    #[test]
    fn format_from_ext_canonical() {
        assert_eq!(format_from_ext("pem"), Some(CertFormat::Pem));
        assert_eq!(format_from_ext("csr"), Some(CertFormat::Pem));
        assert_eq!(format_from_ext("crl"), Some(CertFormat::Pem));
        assert_eq!(format_from_ext("key"), Some(CertFormat::Pem));
        assert_eq!(format_from_ext("pub"), Some(CertFormat::Pem));
        // `.crt` / `.cer` are content-sniffed, not name-routed.
        assert_eq!(format_from_ext("crt"), None);
        assert_eq!(format_from_ext("cer"), None);
        assert_eq!(format_from_ext("txt"), None);
        assert_eq!(format_from_ext("rs"), None);
    }

    #[test]
    fn sniff_pem_header() {
        assert!(sniff_pem("-----BEGIN CERTIFICATE-----\nMIID..."));
        assert!(sniff_pem("   \n  -----BEGIN PRIVATE KEY-----\n..."));
        assert!(!sniff_pem("foo\n-----BEGIN CERTIFICATE-----"));
    }

    #[test]
    fn sniff_pem_ssh_pubkey() {
        assert!(sniff_pem("ssh-rsa AAAAB3NzaC1yc2E... user@host"));
        assert!(sniff_pem("ssh-ed25519 AAAAC3Nz... me@laptop\n"));
        assert!(sniff_pem("ecdsa-sha2-nistp256 AAAAE2VjZHN... user\n"));
        // The prefix alone (no key body) doesn't count.
        assert!(!sniff_pem("ssh-rsa "));
        // Plain text starting with letters doesn't match.
        assert!(!sniff_pem("ssh-keygen output here"));
    }

    #[test]
    fn format_from_ext_der() {
        assert_eq!(format_from_ext("der"), Some(CertFormat::Der));
    }

    #[test]
    fn sniff_der_accepts_real_cert() {
        assert!(sniff_der(&der_fixture()));
    }

    #[test]
    fn sniff_der_rejects_non_cert() {
        // PEM text leads with `-----`, not the DER SEQUENCE tag.
        assert!(!sniff_der(b"-----BEGIN CERTIFICATE-----\nMIID..."));
        // Right tag bytes, but not a decodable certificate.
        assert!(!sniff_der(&[0x30, 0x82, 0x00, 0x05, 1, 2, 3, 4, 5]));
        // Truncated cert (header only) doesn't fully decode.
        assert!(!sniff_der(&der_fixture()[..16]));
        assert!(!sniff_der(b""));
    }

    #[test]
    fn format_from_ext_jwk() {
        assert_eq!(format_from_ext("jwk"), Some(CertFormat::Jwk));
        assert_eq!(format_from_ext("jwks"), Some(CertFormat::Jwk));
    }

    #[test]
    fn sniff_jwk_accepts_key_and_set_rejects_plain_json() {
        let jwk: serde_json::Value = serde_json::json!({"kty": "EC", "crv": "P-256"});
        let set: serde_json::Value = serde_json::json!({"keys": [{"kty": "RSA"}]});
        let plain: serde_json::Value = serde_json::json!({"name": "x", "kty": "unknown"});
        let not_keys: serde_json::Value = serde_json::json!({"keys": [{"id": 1}]});
        assert!(sniff_jwk(&jwk));
        assert!(sniff_jwk(&set));
        assert!(!sniff_jwk(&plain));
        assert!(!sniff_jwk(&not_keys));
    }
}
