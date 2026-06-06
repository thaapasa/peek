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
    let mut obj = serde_json::json!({
        "source_label": info.source_label,
    });
    let entries: Vec<serde_json::Value> = info.entries.iter().map(entry_json).collect();
    obj["entries"] = serde_json::json!(entries);
    if !info.parse_errors.is_empty() {
        obj["parse_errors"] = serde_json::json!(info.parse_errors);
    }
    ("cert", obj)
}

fn entry_json(entry: &CertEntry) -> serde_json::Value {
    match entry {
        CertEntry::Certificate(c) => cert_json(c),
        CertEntry::CertificateRequest(c) => csr_json(c),
        CertEntry::CertificateRevocationList(c) => crl_json(c),
        CertEntry::PrivateKey(k) => key_json(k, "private_key"),
        CertEntry::PublicKey(k) => key_json(k, "public_key"),
        CertEntry::SshPublicKey(k) => ssh_pubkey_json(k),
        CertEntry::JsonWebKey(k) => jwk_json(k),
        CertEntry::Unknown(u) => serde_json::json!({
            "kind": "unknown",
            "label": u.label,
            "der_bytes": u.der_bytes,
        }),
    }
}

fn cert_json(c: &CertificateEntry) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "kind": "certificate",
        "label": c.label,
        "version": c.version,
        "subject": c.subject,
        "issuer": c.issuer,
        "serial_hex": c.serial_hex,
        "not_before": c.not_before,
        "not_after": c.not_after,
        "key_algorithm": c.key_algorithm,
        "signature_algorithm": c.signature_algorithm,
        "fingerprint_sha1": c.fingerprint_sha1,
        "fingerprint_sha256": c.fingerprint_sha256,
        "is_ca": c.is_ca,
        "self_signed": c.self_signed,
    });
    if let Some(days) = c.days_remaining {
        obj["days_remaining"] = serde_json::json!(days);
    }
    if let Some(bits) = c.key_size_bits {
        obj["key_size_bits"] = serde_json::json!(bits);
    }
    if !c.san_dns.is_empty() {
        obj["san_dns"] = serde_json::json!(c.san_dns);
    }
    if !c.san_ip.is_empty() {
        obj["san_ip"] = serde_json::json!(c.san_ip);
    }
    if !c.san_email.is_empty() {
        obj["san_email"] = serde_json::json!(c.san_email);
    }
    if !c.san_uri.is_empty() {
        obj["san_uri"] = serde_json::json!(c.san_uri);
    }
    if !c.key_usages.is_empty() {
        obj["key_usages"] = serde_json::json!(c.key_usages);
    }
    if !c.extended_key_usages.is_empty() {
        obj["extended_key_usages"] = serde_json::json!(c.extended_key_usages);
    }
    obj
}

fn csr_json(c: &CsrEntry) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "kind": "csr",
        "label": c.label,
        "subject": c.subject,
        "key_algorithm": c.key_algorithm,
        "signature_algorithm": c.signature_algorithm,
    });
    if let Some(bits) = c.key_size_bits {
        obj["key_size_bits"] = serde_json::json!(bits);
    }
    if !c.san_dns.is_empty() {
        obj["san_dns"] = serde_json::json!(c.san_dns);
    }
    if !c.san_ip.is_empty() {
        obj["san_ip"] = serde_json::json!(c.san_ip);
    }
    if !c.san_email.is_empty() {
        obj["san_email"] = serde_json::json!(c.san_email);
    }
    if !c.san_uri.is_empty() {
        obj["san_uri"] = serde_json::json!(c.san_uri);
    }
    obj
}

fn crl_json(c: &CrlEntry) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "kind": "crl",
        "label": c.label,
        "issuer": c.issuer,
        "this_update": c.this_update,
        "revoked_count": c.revoked_count,
        "signature_algorithm": c.signature_algorithm,
    });
    if let Some(ref next) = c.next_update {
        obj["next_update"] = serde_json::json!(next);
    }
    obj
}

fn key_json(k: &KeyEntry, kind: &str) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "kind": kind,
        "label": k.label,
        "key_type": key_type_token(&k.key_type),
    });
    if let KeyType::Ec(curve) = &k.key_type
        && !curve.is_empty()
    {
        obj["curve"] = serde_json::json!(curve);
    }
    if let Some(bits) = k.key_size_bits {
        obj["key_size_bits"] = serde_json::json!(bits);
    }
    obj
}

fn ssh_pubkey_json(k: &SshPubKeyEntry) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "kind": "ssh_public_key",
        "algorithm": k.algorithm,
        "fingerprint_sha256": k.fingerprint_sha256,
    });
    if let Some(bits) = k.bits {
        obj["bits"] = serde_json::json!(bits);
    }
    if !k.comment.is_empty() {
        obj["comment"] = serde_json::json!(k.comment);
    }
    obj
}

fn jwk_json(k: &JwkEntry) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "kind": "json_web_key",
        "kty": k.kty,
    });
    if let Some(ref crv) = k.crv {
        obj["crv"] = serde_json::json!(crv);
    }
    if let Some(ref alg) = k.alg {
        obj["alg"] = serde_json::json!(alg);
    }
    if let Some(ref use_) = k.use_ {
        obj["use"] = serde_json::json!(use_);
    }
    if let Some(ref kid) = k.kid {
        obj["kid"] = serde_json::json!(kid);
    }
    if !k.key_ops.is_empty() {
        obj["key_ops"] = serde_json::json!(k.key_ops);
    }
    if let Some(bits) = k.key_size_bits {
        obj["key_size_bits"] = serde_json::json!(bits);
    }
    if let Some(ref tp) = k.thumbprint {
        obj["thumbprint"] = serde_json::json!(tp);
    }
    obj
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
