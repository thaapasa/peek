//! Cert / key container format. Only PEM ships in the first cut —
//! covers `-----BEGIN …-----` blocks (any label) and bare OpenSSH
//! public-key text (single-line `ssh-rsa AAAA…` / `ecdsa-sha2-… AAAA…`
//! / `ssh-ed25519 AAAA…`). DER / PKCS#12 are tracked in
//! `docs/planned.md` and not yet wired.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertFormat {
    /// PEM container (or OpenSSH public-key text). Decode is best
    /// effort per block — unknown labels surface as `Unknown` entries.
    Pem,
}
