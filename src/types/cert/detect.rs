//! Detection contributions for cert/key files. The `format_from_ext`
//! entry covers extension routing in [`classify_by_name`]; the
//! `sniff_pem` entry covers content sniffing for unnamed sources
//! (stdin, archive entries with the wrong/missing extension).
//!
//! [`classify_by_name`]: crate::input::detect

use crate::types::cert::format::CertFormat;

/// Map a lowercased filename extension to the cert/key container
/// format. Only extensions that are unambiguously text (PEM-encoded)
/// are routed here — `.crt` / `.cer` are deliberately left out
/// because they routinely carry raw DER as well. PEM files with
/// those extensions still resolve via the [`sniff_pem`] content
/// path; DER files fall through to the binary viewer where the hex
/// view is more useful than a mojibake source dump.
pub fn format_from_ext(ext: &str) -> Option<CertFormat> {
    matches!(ext, "pem" | "csr" | "crl" | "key" | "p7b" | "p7c" | "pub").then_some(CertFormat::Pem)
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
}
