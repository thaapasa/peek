//! Detection contributions for cert/key files. The `format_from_ext`
//! entry covers extension routing in [`classify_by_name`]; the
//! `sniff_pem` entry covers content sniffing for unnamed sources
//! (stdin, archive entries with the wrong/missing extension).
//!
//! [`classify_by_name`]: crate::input::detect

use x509_parser::prelude::{FromDer, X509Certificate};

use crate::types::cert::format::CertFormat;

/// Map a lowercased filename extension to the cert/key container
/// format. Text (PEM) extensions route to `Pem`; the unambiguous binary
/// `.der` routes to `Der`. `.crt` / `.cer` are deliberately left out —
/// they routinely carry *either* encoding, so they resolve by content:
/// a PEM header via [`sniff_pem`], raw DER via [`sniff_der`].
pub fn format_from_ext(ext: &str) -> Option<CertFormat> {
    match ext {
        "pem" | "csr" | "crl" | "key" | "p7b" | "p7c" | "pub" => Some(CertFormat::Pem),
        "der" => Some(CertFormat::Der),
        _ => None,
    }
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

    fn der_fixture() -> Vec<u8> {
        let path =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-data/cert-rsa.der");
        std::fs::read(path).expect("cert-rsa.der fixture")
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
    fn gather_der_decodes_certificate() {
        use crate::types::cert::info::CertEntry;
        let info = crate::types::cert::info_gather::gather_der(&der_fixture());
        assert!(info.text.is_none());
        assert_eq!(info.source_label, "DER");
        assert_eq!(info.entries.len(), 1);
        assert!(info.parse_errors.is_empty());
        match &info.entries[0] {
            CertEntry::Certificate(c) => assert!(!c.subject.is_empty()),
            other => panic!("expected Certificate, got {:?}", entry_kind(other)),
        }
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
            Unknown(_) => "Unknown",
        }
    }
}
