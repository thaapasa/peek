//! Cert/key info shape: text-stats sidecar plus a list of decoded
//! entries (one PEM file may carry many — a fullchain is N
//! certificates; an `id_rsa` is one key block; an `authorized_keys`
//! is a stream of SSH pubkey lines).

use crate::types::text::info::TextStats;

pub struct CertInfo {
    /// Text stats for the source — `None` for a raw DER file, which is
    /// binary and has no line / word / encoding facts to report.
    pub text: Option<TextStats>,
    /// Container label for the entries section header (`PEM` / `DER`).
    pub source_label: &'static str,
    pub entries: Vec<CertEntry>,
    /// Best-effort decode errors. One per failed PEM block / SSH
    /// pubkey line — rendered as a Warning row so a malformed entry
    /// doesn't suppress the rest of the section.
    pub parse_errors: Vec<String>,
}

/// One decoded item from the source. Covers every variant the first
/// cut surfaces (X.509 cert / CSR / CRL / private key / public key /
/// OpenSSH public key). Unrecognised PEM labels land in
/// `Unknown` with the raw label so the user still sees what the file
/// declared.
pub enum CertEntry {
    // Heavier variants (CertificateEntry, CsrEntry, CrlEntry) are
    // boxed to keep the enum compact — the Vec<CertEntry> for a
    // full chain stays small even when most entries are keys/SSH
    // pubkeys, and the indirection cost is irrelevant for an info
    // section rendered once per file.
    Certificate(Box<CertificateEntry>),
    CertificateRequest(Box<CsrEntry>),
    CertificateRevocationList(Box<CrlEntry>),
    PrivateKey(KeyEntry),
    PublicKey(KeyEntry),
    SshPublicKey(SshPubKeyEntry),
    JsonWebKey(JwkEntry),
    Unknown(UnknownEntry),
}

pub struct CertificateEntry {
    /// PEM label (`CERTIFICATE`, `TRUSTED CERTIFICATE`, …).
    pub label: String,
    pub subject: String,
    pub issuer: String,
    /// Colon-separated hex (`01:23:AB:…`).
    pub serial_hex: String,
    /// ISO 8601 UTC (`2026-01-15T00:00:00Z`) — formatted by the
    /// info_gather pass, not the renderer.
    pub not_before: String,
    pub not_after: String,
    /// Days from now (clamped at 99,999 either direction); negative =
    /// already expired. `None` when validity timestamps couldn't be
    /// converted to wall-clock days.
    pub days_remaining: Option<i64>,
    pub san_dns: Vec<String>,
    pub san_ip: Vec<String>,
    pub san_email: Vec<String>,
    pub san_uri: Vec<String>,
    pub key_algorithm: String,
    /// RSA modulus bits / EC curve bits / Ed25519 = 256 / `None`
    /// when the format doesn't expose a useful bit count.
    pub key_size_bits: Option<usize>,
    pub signature_algorithm: String,
    /// Colon-separated hex SHA-1 fingerprint over the DER (`AB:CD:…`).
    pub fingerprint_sha1: String,
    /// Colon-separated hex SHA-256 fingerprint over the DER.
    pub fingerprint_sha256: String,
    pub is_ca: bool,
    /// `true` when subject == issuer at DER level — heuristic but
    /// matches what `openssl x509` displays.
    pub self_signed: bool,
    pub version: u32,
    pub key_usages: Vec<String>,
    pub extended_key_usages: Vec<String>,
}

pub struct CsrEntry {
    pub label: String,
    pub subject: String,
    pub san_dns: Vec<String>,
    pub san_ip: Vec<String>,
    pub san_email: Vec<String>,
    pub san_uri: Vec<String>,
    pub key_algorithm: String,
    pub key_size_bits: Option<usize>,
    pub signature_algorithm: String,
}

pub struct CrlEntry {
    pub label: String,
    pub issuer: String,
    pub this_update: String,
    pub next_update: Option<String>,
    pub revoked_count: usize,
    pub signature_algorithm: String,
}

pub struct KeyEntry {
    /// PEM label (`RSA PRIVATE KEY`, `EC PRIVATE KEY`, `PRIVATE KEY`,
    /// `PUBLIC KEY`, `OPENSSH PRIVATE KEY`, …).
    pub label: String,
    pub key_type: KeyType,
    /// Best-effort bit size — RSA modulus, EC curve bits, Ed25519 =
    /// 256. `None` if undecidable from the PEM body alone (encrypted
    /// PKCS#8, PKCS#12, opaque DSA, etc.).
    pub key_size_bits: Option<usize>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum KeyType {
    Rsa,
    /// Carries the curve name when known (`prime256v1`, `secp384r1`,
    /// `secp521r1`); empty string when only the family is recognised.
    Ec(String),
    Ed25519,
    Dsa,
    /// Opaque or encrypted — `label` still tells the user what the
    /// file declared even when the body can't be inspected.
    Other,
}

pub struct SshPubKeyEntry {
    pub algorithm: String,
    pub bits: Option<usize>,
    pub comment: String,
    /// `SHA256:base64`-formatted fingerprint, matching `ssh-keygen
    /// -l` output for the same key.
    pub fingerprint_sha256: String,
}

/// One JSON Web Key (RFC 7517). A JWK Set yields one entry per member of
/// its `keys` array; a bare JWK yields a single entry. Fields not present
/// in the key are `None` / empty.
pub struct JwkEntry {
    /// Key type (`RSA` / `EC` / `oct` / `OKP`) — the one required member.
    pub kty: String,
    /// Curve (`P-256`, `Ed25519`, …) for EC / OKP keys.
    pub crv: Option<String>,
    /// Intended algorithm (`RS256`, `ES256`, …).
    pub alg: Option<String>,
    /// Public-key use (`sig` / `enc`).
    pub use_: Option<String>,
    /// Key ID.
    pub kid: Option<String>,
    /// Permitted operations (`sign`, `verify`, …).
    pub key_ops: Vec<String>,
    /// Best-effort key size — RSA modulus bits, EC / OKP curve bits, or
    /// `oct` secret bits. `None` when the material to size it is absent.
    pub key_size_bits: Option<usize>,
    /// RFC 7638 thumbprint (`base64url(SHA-256(canonical JWK))`),
    /// prefixed `SHA-256:`. `None` when the required members are missing.
    pub thumbprint: Option<String>,
}

pub struct UnknownEntry {
    /// The raw PEM label so the user sees what the source declared.
    pub label: String,
    /// DER body size after base64-decode; useful sanity check that
    /// the block is non-empty even though we couldn't classify it.
    pub der_bytes: usize,
}
