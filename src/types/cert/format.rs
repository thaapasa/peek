//! Cert / key container format. PEM covers `-----BEGIN …-----` blocks
//! (any label) and bare OpenSSH public-key text (single-line `ssh-rsa
//! AAAA…` / `ecdsa-sha2-… AAAA…` / `ssh-ed25519 AAAA…`). DER is the raw
//! ASN.1 binary form an X.509 certificate carries without the base64
//! armour — decoded through the same path, just minus the PEM unwrap.
//! PKCS#12 is tracked in `docs/planned.md` and not yet wired.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertFormat {
    /// PEM container (or OpenSSH public-key text). Decode is best
    /// effort per block — unknown labels surface as `Unknown` entries.
    Pem,
    /// Raw DER (binary ASN.1) — a single X.509 certificate / CRL / CSR /
    /// key with no base64 armour and no label. The decoder tries each
    /// structure in turn to recover the kind.
    Der,
}
