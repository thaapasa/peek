use crate::info::{paint_count, push_field, push_section_header};
use crate::theme::PeekTheme;
use crate::types::cert::info::{
    CertEntry, CertInfo, CertificateEntry, CrlEntry, CsrEntry, JwkEntry, KeyEntry, KeyType,
    SshPubKeyEntry, UnknownEntry,
};
use crate::types::text::info_render::push_text_stats;

/// Render the cert section. Standard Content (text stats) header
/// stays so the user gets the same line/word/encoding facts they get
/// for any text file; the cert-specific section appears below.
pub fn render_section(lines: &mut Vec<String>, info: &CertInfo, theme: &PeekTheme) {
    // Text stats only apply to a text (PEM) source; a raw DER file is
    // binary, so its `text` is `None` and the Content section is skipped.
    if let Some(text) = &info.text {
        lines.push(String::new());
        push_section_header(lines, "Content", theme);
        push_text_stats(lines, text, theme);
    }

    if info.entries.is_empty() && info.parse_errors.is_empty() {
        return;
    }

    lines.push(String::new());
    push_section_header(lines, info.source_label, theme);
    push_field(
        lines,
        "Entries",
        &paint_count(info.entries.len(), theme),
        theme,
    );

    for (i, entry) in info.entries.iter().enumerate() {
        lines.push(String::new());
        let title = entry_title(entry, i + 1);
        lines.push(format!(
            "{} {}",
            theme.paint_muted("\u{2500}\u{2500}"),
            theme.paint_heading(&title),
        ));
        render_entry(lines, entry, theme);
    }

    for err in &info.parse_errors {
        lines.push(String::new());
        push_field(
            lines,
            "Parse error",
            &theme.paint(err, theme.warning),
            theme,
        );
    }
}

fn entry_title(entry: &CertEntry, index: usize) -> String {
    let kind = match entry {
        CertEntry::Certificate(_) => "Certificate",
        CertEntry::CertificateRequest(_) => "CSR",
        CertEntry::CertificateRevocationList(_) => "CRL",
        CertEntry::PrivateKey(_) => "Private Key",
        CertEntry::PublicKey(_) => "Public Key",
        CertEntry::SshPublicKey(_) => "SSH Public Key",
        CertEntry::JsonWebKey(_) => "JSON Web Key",
        CertEntry::Unknown(_) => "Unknown",
    };
    format!("{kind} #{index}")
}

fn render_entry(lines: &mut Vec<String>, entry: &CertEntry, theme: &PeekTheme) {
    match entry {
        CertEntry::Certificate(c) => render_cert(lines, c, theme),
        CertEntry::CertificateRequest(c) => render_csr(lines, c, theme),
        CertEntry::CertificateRevocationList(c) => render_crl(lines, c, theme),
        CertEntry::PrivateKey(k) => render_key(lines, k, theme, false),
        CertEntry::PublicKey(k) => render_key(lines, k, theme, true),
        CertEntry::SshPublicKey(k) => render_ssh_pubkey(lines, k, theme),
        CertEntry::JsonWebKey(k) => render_jwk(lines, k, theme),
        CertEntry::Unknown(u) => render_unknown(lines, u, theme),
    }
}

fn render_jwk(lines: &mut Vec<String>, k: &JwkEntry, theme: &PeekTheme) {
    let kind = match &k.crv {
        Some(crv) if !crv.is_empty() => format!("{} ({crv})", k.kty),
        _ => k.kty.clone(),
    };
    push_field(lines, "Type", &theme.paint_value(&kind), theme);
    if let Some(bits) = k.key_size_bits {
        push_field(lines, "Bits", &theme.paint_value(&bits.to_string()), theme);
    }
    if let Some(alg) = &k.alg {
        push_field(lines, "Algorithm", &theme.paint_value(alg), theme);
    }
    if let Some(use_) = &k.use_ {
        push_field(lines, "Use", &theme.paint_value(use_), theme);
    }
    if !k.key_ops.is_empty() {
        push_field(
            lines,
            "Key Ops",
            &theme.paint_value(&k.key_ops.join(", ")),
            theme,
        );
    }
    if let Some(kid) = &k.kid {
        push_field(lines, "Key ID", &theme.paint_muted(kid), theme);
    }
    if let Some(tp) = &k.thumbprint {
        push_field(lines, "Thumbprint", &theme.paint_muted(tp), theme);
    }
}

fn render_cert(lines: &mut Vec<String>, c: &CertificateEntry, theme: &PeekTheme) {
    push_field(lines, "Label", &theme.paint_muted(&c.label), theme);
    push_field(
        lines,
        "Version",
        &theme.paint_value(&c.version.to_string()),
        theme,
    );
    push_field(lines, "Subject", &theme.paint_value(&c.subject), theme);
    push_field(lines, "Issuer", &theme.paint_value(&c.issuer), theme);
    push_field(lines, "Serial", &theme.paint_muted(&c.serial_hex), theme);
    push_field(
        lines,
        "Not Before",
        &theme.paint_value(&c.not_before),
        theme,
    );
    push_field(lines, "Not After", &theme.paint_value(&c.not_after), theme);
    if let Some(days) = c.days_remaining {
        let (label, color) = if days < 0 {
            (format!("expired {} days ago", -days), theme.warning)
        } else if days <= 30 {
            (format!("{days} days"), theme.warning)
        } else {
            (format!("{days} days"), theme.value)
        };
        push_field(lines, "Days Left", &theme.paint(&label, color), theme);
    }
    push_key_algo(lines, &c.key_algorithm, c.key_size_bits, theme);
    push_field(
        lines,
        "Signature",
        &theme.paint_value(&c.signature_algorithm),
        theme,
    );

    push_sans(
        lines,
        &c.san_dns,
        &c.san_ip,
        &c.san_email,
        &c.san_uri,
        theme,
    );

    if c.is_ca {
        push_field(lines, "CA", &theme.paint_value("yes"), theme);
    }
    if c.self_signed {
        push_field(lines, "Self-Signed", &theme.paint_value("yes"), theme);
    }

    if !c.key_usages.is_empty() {
        push_field(
            lines,
            "Key Usage",
            &theme.paint_value(&c.key_usages.join(", ")),
            theme,
        );
    }
    if !c.extended_key_usages.is_empty() {
        push_field(
            lines,
            "Ext Key Usage",
            &theme.paint_value(&c.extended_key_usages.join(", ")),
            theme,
        );
    }

    push_field(
        lines,
        "SHA-1",
        &theme.paint_muted(&c.fingerprint_sha1),
        theme,
    );
    push_field(
        lines,
        "SHA-256",
        &theme.paint_muted(&c.fingerprint_sha256),
        theme,
    );
}

fn render_csr(lines: &mut Vec<String>, c: &CsrEntry, theme: &PeekTheme) {
    push_field(lines, "Label", &theme.paint_muted(&c.label), theme);
    push_field(lines, "Subject", &theme.paint_value(&c.subject), theme);
    push_key_algo(lines, &c.key_algorithm, c.key_size_bits, theme);
    push_field(
        lines,
        "Signature",
        &theme.paint_value(&c.signature_algorithm),
        theme,
    );
    push_sans(
        lines,
        &c.san_dns,
        &c.san_ip,
        &c.san_email,
        &c.san_uri,
        theme,
    );
}

fn render_crl(lines: &mut Vec<String>, c: &CrlEntry, theme: &PeekTheme) {
    push_field(lines, "Label", &theme.paint_muted(&c.label), theme);
    push_field(lines, "Issuer", &theme.paint_value(&c.issuer), theme);
    push_field(
        lines,
        "This Update",
        &theme.paint_value(&c.this_update),
        theme,
    );
    if let Some(next) = &c.next_update {
        push_field(lines, "Next Update", &theme.paint_value(next), theme);
    }
    push_field(
        lines,
        "Revoked",
        &paint_count(c.revoked_count, theme),
        theme,
    );
    push_field(
        lines,
        "Signature",
        &theme.paint_value(&c.signature_algorithm),
        theme,
    );
}

fn render_key(lines: &mut Vec<String>, k: &KeyEntry, theme: &PeekTheme, _public: bool) {
    push_field(lines, "Label", &theme.paint_muted(&k.label), theme);
    push_field(
        lines,
        "Type",
        &theme.paint_value(&key_type_label(&k.key_type)),
        theme,
    );
    if let Some(bits) = k.key_size_bits {
        push_field(lines, "Bits", &theme.paint_value(&bits.to_string()), theme);
    }
}

fn render_ssh_pubkey(lines: &mut Vec<String>, k: &SshPubKeyEntry, theme: &PeekTheme) {
    push_field(lines, "Algorithm", &theme.paint_value(&k.algorithm), theme);
    if let Some(bits) = k.bits {
        push_field(lines, "Bits", &theme.paint_value(&bits.to_string()), theme);
    }
    if !k.comment.is_empty() {
        push_field(lines, "Comment", &theme.paint_muted(&k.comment), theme);
    }
    push_field(
        lines,
        "SHA-256",
        &theme.paint_muted(&k.fingerprint_sha256),
        theme,
    );
}

fn render_unknown(lines: &mut Vec<String>, u: &UnknownEntry, theme: &PeekTheme) {
    push_field(lines, "Label", &theme.paint_muted(&u.label), theme);
    push_field(lines, "DER Bytes", &paint_count(u.der_bytes, theme), theme);
}

fn push_key_algo(lines: &mut Vec<String>, algorithm: &str, bits: Option<usize>, theme: &PeekTheme) {
    let label = match bits {
        Some(b) => format!("{algorithm} ({b} bit)"),
        None => algorithm.to_string(),
    };
    push_field(lines, "Public Key", &theme.paint_value(&label), theme);
}

fn push_sans(
    lines: &mut Vec<String>,
    dns: &[String],
    ip: &[String],
    email: &[String],
    uri: &[String],
    theme: &PeekTheme,
) {
    push_san(lines, "SAN DNS", dns, theme);
    push_san(lines, "SAN IP", ip, theme);
    push_san(lines, "SAN Email", email, theme);
    push_san(lines, "SAN URI", uri, theme);
}

fn push_san(lines: &mut Vec<String>, label: &str, names: &[String], theme: &PeekTheme) {
    if names.is_empty() {
        return;
    }
    push_field(lines, label, &theme.paint_value(&names.join(", ")), theme);
}

fn key_type_label(kt: &KeyType) -> String {
    match kt {
        KeyType::Rsa => "RSA".to_string(),
        KeyType::Ec(curve) if curve.is_empty() => "EC".to_string(),
        KeyType::Ec(curve) => format!("EC ({curve})"),
        KeyType::Ed25519 => "Ed25519".to_string(),
        KeyType::Dsa => "DSA".to_string(),
        KeyType::Other => "opaque".to_string(),
    }
}

/// Typed `--info --json` encoding of the cert section. Each decoded entry
/// becomes an object tagged with a stable lowercase `kind` token; dates and
/// fingerprints are emitted as the already-formatted strings the gather pass
/// produced (ISO-8601 timestamps, colon-hex digests). Counts and bit sizes
/// stay raw numbers. Optional fields are omitted when absent; empty SAN /
/// key-usage lists are omitted.
pub fn json_section(info: &CertInfo) -> (&'static str, serde_json::Value) {
    let view = CertJsonView {
        source_label: info.source_label,
        entries: info.entries.iter().map(CertEntryJson::from_entry).collect(),
        parse_errors: info.parse_errors.clone(),
    };
    (
        "cert",
        serde_json::to_value(view).expect("cert info view serializes"),
    )
}

#[derive(serde::Serialize)]
struct CertJsonView {
    source_label: &'static str,
    entries: Vec<CertEntryJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    parse_errors: Vec<String>,
}

/// Internally-tagged entry: the variant name becomes the `kind` token and
/// the inner struct's fields merge alongside it. serde reproduces the
/// per-variant shape that was previously hand-built object by object.
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CertEntryJson {
    // Heavy variants boxed to keep the enum compact — mirrors the domain
    // `CertEntry` and silences clippy's large_enum_variant. `Box<T>`
    // serializes transparently, so the JSON shape is unchanged.
    Certificate(Box<CertJson>),
    Csr(Box<CsrJson>),
    Crl(Box<CrlJson>),
    PrivateKey(KeyJson),
    PublicKey(KeyJson),
    SshPublicKey(SshJson),
    JsonWebKey(JwkJson),
    Unknown(UnknownJson),
}

impl CertEntryJson {
    fn from_entry(entry: &CertEntry) -> Self {
        match entry {
            CertEntry::Certificate(c) => {
                CertEntryJson::Certificate(Box::new(CertJson::from(c.as_ref())))
            }
            CertEntry::CertificateRequest(c) => {
                CertEntryJson::Csr(Box::new(CsrJson::from(c.as_ref())))
            }
            CertEntry::CertificateRevocationList(c) => {
                CertEntryJson::Crl(Box::new(CrlJson::from(c.as_ref())))
            }
            CertEntry::PrivateKey(k) => CertEntryJson::PrivateKey(KeyJson::from(k)),
            CertEntry::PublicKey(k) => CertEntryJson::PublicKey(KeyJson::from(k)),
            CertEntry::SshPublicKey(k) => CertEntryJson::SshPublicKey(SshJson::from(k)),
            CertEntry::JsonWebKey(k) => CertEntryJson::JsonWebKey(JwkJson::from(k)),
            CertEntry::Unknown(u) => CertEntryJson::Unknown(UnknownJson {
                label: u.label.clone(),
                der_bytes: u.der_bytes,
            }),
        }
    }
}

#[derive(serde::Serialize)]
struct CertJson {
    label: String,
    version: u32,
    subject: String,
    issuer: String,
    serial_hex: String,
    not_before: String,
    not_after: String,
    key_algorithm: String,
    signature_algorithm: String,
    fingerprint_sha1: String,
    fingerprint_sha256: String,
    is_ca: bool,
    self_signed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    days_remaining: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_size_bits: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    san_dns: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    san_ip: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    san_email: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    san_uri: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    key_usages: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    extended_key_usages: Vec<String>,
}

impl From<&CertificateEntry> for CertJson {
    fn from(c: &CertificateEntry) -> Self {
        CertJson {
            label: c.label.clone(),
            version: c.version,
            subject: c.subject.clone(),
            issuer: c.issuer.clone(),
            serial_hex: c.serial_hex.clone(),
            not_before: c.not_before.clone(),
            not_after: c.not_after.clone(),
            key_algorithm: c.key_algorithm.clone(),
            signature_algorithm: c.signature_algorithm.clone(),
            fingerprint_sha1: c.fingerprint_sha1.clone(),
            fingerprint_sha256: c.fingerprint_sha256.clone(),
            is_ca: c.is_ca,
            self_signed: c.self_signed,
            days_remaining: c.days_remaining,
            key_size_bits: c.key_size_bits,
            san_dns: c.san_dns.clone(),
            san_ip: c.san_ip.clone(),
            san_email: c.san_email.clone(),
            san_uri: c.san_uri.clone(),
            key_usages: c.key_usages.clone(),
            extended_key_usages: c.extended_key_usages.clone(),
        }
    }
}

#[derive(serde::Serialize)]
struct CsrJson {
    label: String,
    subject: String,
    key_algorithm: String,
    signature_algorithm: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_size_bits: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    san_dns: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    san_ip: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    san_email: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    san_uri: Vec<String>,
}

impl From<&CsrEntry> for CsrJson {
    fn from(c: &CsrEntry) -> Self {
        CsrJson {
            label: c.label.clone(),
            subject: c.subject.clone(),
            key_algorithm: c.key_algorithm.clone(),
            signature_algorithm: c.signature_algorithm.clone(),
            key_size_bits: c.key_size_bits,
            san_dns: c.san_dns.clone(),
            san_ip: c.san_ip.clone(),
            san_email: c.san_email.clone(),
            san_uri: c.san_uri.clone(),
        }
    }
}

#[derive(serde::Serialize)]
struct CrlJson {
    label: String,
    issuer: String,
    this_update: String,
    revoked_count: usize,
    signature_algorithm: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_update: Option<String>,
}

impl From<&CrlEntry> for CrlJson {
    fn from(c: &CrlEntry) -> Self {
        CrlJson {
            label: c.label.clone(),
            issuer: c.issuer.clone(),
            this_update: c.this_update.clone(),
            revoked_count: c.revoked_count,
            signature_algorithm: c.signature_algorithm.clone(),
            next_update: c.next_update.clone(),
        }
    }
}

#[derive(serde::Serialize)]
struct KeyJson {
    label: String,
    key_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    curve: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_size_bits: Option<usize>,
}

impl From<&KeyEntry> for KeyJson {
    fn from(k: &KeyEntry) -> Self {
        let curve = match &k.key_type {
            KeyType::Ec(c) if !c.is_empty() => Some(c.clone()),
            _ => None,
        };
        KeyJson {
            label: k.label.clone(),
            key_type: key_type_token(&k.key_type),
            curve,
            key_size_bits: k.key_size_bits,
        }
    }
}

#[derive(serde::Serialize)]
struct SshJson {
    algorithm: String,
    fingerprint_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    bits: Option<usize>,
    #[serde(skip_serializing_if = "String::is_empty")]
    comment: String,
}

impl From<&SshPubKeyEntry> for SshJson {
    fn from(k: &SshPubKeyEntry) -> Self {
        SshJson {
            algorithm: k.algorithm.clone(),
            fingerprint_sha256: k.fingerprint_sha256.clone(),
            bits: k.bits,
            comment: k.comment.clone(),
        }
    }
}

#[derive(serde::Serialize)]
struct JwkJson {
    kty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    crv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    alg: Option<String>,
    #[serde(rename = "use", skip_serializing_if = "Option::is_none")]
    use_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kid: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    key_ops: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_size_bits: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thumbprint: Option<String>,
}

impl From<&JwkEntry> for JwkJson {
    fn from(k: &JwkEntry) -> Self {
        JwkJson {
            kty: k.kty.clone(),
            crv: k.crv.clone(),
            alg: k.alg.clone(),
            use_: k.use_.clone(),
            kid: k.kid.clone(),
            key_ops: k.key_ops.clone(),
            key_size_bits: k.key_size_bits,
            thumbprint: k.thumbprint.clone(),
        }
    }
}

#[derive(serde::Serialize)]
struct UnknownJson {
    label: String,
    der_bytes: usize,
}

fn key_type_token(kt: &KeyType) -> &'static str {
    match kt {
        KeyType::Rsa => "rsa",
        KeyType::Ec(_) => "ec",
        KeyType::Ed25519 => "ed25519",
        KeyType::Dsa => "dsa",
        KeyType::Other => "other",
    }
}
