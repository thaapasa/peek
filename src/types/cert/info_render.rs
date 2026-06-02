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
