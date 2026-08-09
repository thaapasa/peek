//! The cert section. Each decoded entry is an irregular block — variant-
//! dispatched layout, several rows whose print and JSON forms diverge (a
//! `Days Left` line vs a raw `days_remaining`, a combined `Public Key` row vs
//! separate `key_algorithm` + `key_size_bits`, a join-string SAN row vs an
//! array). The `#[derive(InfoView)]` can't express that, so each variant builds
//! one [`InfoRow`] list that drives *both* print and JSON: [`push_rows`] for
//! the themed lines, [`rows_to_json`] for the object. One builder per variant
//! replaces the former parallel `render_*` / `*Json` hierarchies.
//!
//! The per-entry headers aren't standard section rules, so the bodies are
//! carried verbatim as `Line` nodes under a hand-built header.

use peek_theme::PeekTheme;
use serde_json::json;

use crate::info::{
    InfoNode, InfoRow, Role, Value, paint_count, push_entry, push_parse_errors, render_info,
    rows_to_json,
};
use crate::types::cert::info::{
    CertEntry, CertInfo, CertificateEntry, CrlEntry, CsrEntry, JwkEntry, KeyEntry, KeyType,
    SshPubKeyEntry, UnknownEntry,
};
use crate::types::text::info_render::TextView;

/// Render the cert section through the shared node tree. A Content block (text
/// stats) leads when the source is text; the cert-specific block follows.
pub fn render_section(lines: &mut Vec<String>, info: &CertInfo, theme: &PeekTheme) {
    render_info(lines, &CertView(info), theme);
}

/// View wrapper so the cert section flows through [`render_info`] like every
/// other type. `info_nodes` builds the tree; JSON stays in `json_section`.
struct CertView<'a>(&'a CertInfo);

impl crate::info::InfoView for CertView<'_> {
    fn info_nodes(&self, theme: &PeekTheme) -> Vec<InfoNode> {
        let info = self.0;
        let mut nodes = Vec::new();

        // Text stats only apply to a text (PEM) source; a raw DER file's
        // `text` is `None` and the Content block is skipped.
        if let Some(text) = &info.text {
            nodes.extend(TextView::from(text).info_nodes(theme));
        }

        if info.entries.is_empty() && info.parse_errors.is_empty() {
            return nodes;
        }

        nodes.push(InfoNode::Block {
            title: info.source_label.to_string(),
            body: vec![InfoNode::Row {
                label: "Entries".into(),
                value: paint_count(info.entries.len(), theme),
            }],
        });

        for (i, entry) in info.entries.iter().enumerate() {
            // One row list drives the print body here and the JSON in
            // `json_section`.
            push_entry(
                &mut nodes,
                theme,
                &entry_title(entry, i + 1),
                &entry_rows(entry),
            );
        }

        push_parse_errors(&mut nodes, theme, &info.parse_errors);
        nodes
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

/// Typed `--info --json` encoding of the cert section: each entry's row list
/// becomes a `kind`-tagged object, dates and fingerprints emitted as the
/// already-formatted gather strings, counts and bit sizes as raw numbers.
pub fn json_section(info: &CertInfo) -> (&'static str, serde_json::Value) {
    let entries: Vec<serde_json::Value> = info
        .entries
        .iter()
        .map(|e| serde_json::Value::Object(rows_to_json(&entry_rows(e))))
        .collect();
    let mut obj = serde_json::Map::new();
    obj.insert("source_label".into(), json!(info.source_label));
    obj.insert("entries".into(), serde_json::Value::Array(entries));
    if !info.parse_errors.is_empty() {
        obj.insert("parse_errors".into(), json!(info.parse_errors));
    }
    ("cert", serde_json::Value::Object(obj))
}

/// The one row list per entry — variant-dispatched, feeding both outputs.
fn entry_rows(entry: &CertEntry) -> Vec<InfoRow> {
    match entry {
        CertEntry::Certificate(c) => cert_rows(c),
        CertEntry::CertificateRequest(c) => csr_rows(c),
        CertEntry::CertificateRevocationList(c) => crl_rows(c),
        CertEntry::PrivateKey(k) => key_rows(k, "private_key"),
        CertEntry::PublicKey(k) => key_rows(k, "public_key"),
        CertEntry::SshPublicKey(k) => ssh_rows(k),
        CertEntry::JsonWebKey(k) => jwk_rows(k),
        CertEntry::Unknown(u) => unknown_rows(u),
    }
}

fn cert_rows(c: &CertificateEntry) -> Vec<InfoRow> {
    let mut r = vec![
        InfoRow::json_only("kind", Value::token("certificate")),
        InfoRow::muted("Label", "label", c.label.clone()),
        InfoRow::new("Version", "version", Value::int_plain(c.version as i64)),
        InfoRow::text("Subject", "subject", c.subject.clone()),
        InfoRow::text("Issuer", "issuer", c.issuer.clone()),
        InfoRow::muted("Serial", "serial_hex", c.serial_hex.clone()),
        InfoRow::text("Not Before", "not_before", c.not_before.clone()),
        InfoRow::text("Not After", "not_after", c.not_after.clone()),
    ];
    if let Some(days) = c.days_remaining {
        let (label, warn) = if days < 0 {
            (format!("expired {} days ago", -days), true)
        } else if days <= 30 {
            (format!("{days} days"), true)
        } else {
            (format!("{days} days"), false)
        };
        // One print line; the raw count is its own JSON-only key.
        r.push(InfoRow::print_only(
            "Days Left",
            if warn {
                Value::warn(label)
            } else {
                Value::text(label)
            },
        ));
        r.push(InfoRow::json_int("days_remaining", days));
    }
    // One print row "alg (bits bit)"; JSON splits into two flat keys.
    r.push(InfoRow::print_only(
        "Public Key",
        Value::text(key_algo_label(&c.key_algorithm, c.key_size_bits)),
    ));
    r.push(InfoRow::json_text("key_algorithm", c.key_algorithm.clone()));
    if let Some(b) = c.key_size_bits {
        r.push(InfoRow::json_int("key_size_bits", b as i64));
    }
    r.push(InfoRow::text(
        "Signature",
        "signature_algorithm",
        c.signature_algorithm.clone(),
    ));
    push_list_row(&mut r, "SAN DNS", "san_dns", &c.san_dns);
    push_list_row(&mut r, "SAN IP", "san_ip", &c.san_ip);
    push_list_row(&mut r, "SAN Email", "san_email", &c.san_email);
    push_list_row(&mut r, "SAN URI", "san_uri", &c.san_uri);
    // JSON keeps the bool always; print shows the row only when true.
    r.push(InfoRow::json_bool("is_ca", c.is_ca));
    if c.is_ca {
        r.push(InfoRow::print_only("CA", Value::text("yes")));
    }
    r.push(InfoRow::json_bool("self_signed", c.self_signed));
    if c.self_signed {
        r.push(InfoRow::print_only("Self-Signed", Value::text("yes")));
    }
    push_list_row(&mut r, "Key Usage", "key_usages", &c.key_usages);
    push_list_row(
        &mut r,
        "Ext Key Usage",
        "extended_key_usages",
        &c.extended_key_usages,
    );
    r.push(InfoRow::muted(
        "SHA-1",
        "fingerprint_sha1",
        c.fingerprint_sha1.clone(),
    ));
    r.push(InfoRow::muted(
        "SHA-256",
        "fingerprint_sha256",
        c.fingerprint_sha256.clone(),
    ));
    r
}

fn csr_rows(c: &CsrEntry) -> Vec<InfoRow> {
    let mut r = vec![
        InfoRow::json_only("kind", Value::token("csr")),
        InfoRow::muted("Label", "label", c.label.clone()),
        InfoRow::text("Subject", "subject", c.subject.clone()),
        InfoRow::print_only(
            "Public Key",
            Value::text(key_algo_label(&c.key_algorithm, c.key_size_bits)),
        ),
        InfoRow::json_text("key_algorithm", c.key_algorithm.clone()),
    ];
    if let Some(b) = c.key_size_bits {
        r.push(InfoRow::json_int("key_size_bits", b as i64));
    }
    r.push(InfoRow::text(
        "Signature",
        "signature_algorithm",
        c.signature_algorithm.clone(),
    ));
    push_list_row(&mut r, "SAN DNS", "san_dns", &c.san_dns);
    push_list_row(&mut r, "SAN IP", "san_ip", &c.san_ip);
    push_list_row(&mut r, "SAN Email", "san_email", &c.san_email);
    push_list_row(&mut r, "SAN URI", "san_uri", &c.san_uri);
    r
}

fn crl_rows(c: &CrlEntry) -> Vec<InfoRow> {
    let mut r = vec![
        InfoRow::json_only("kind", Value::token("crl")),
        InfoRow::muted("Label", "label", c.label.clone()),
        InfoRow::text("Issuer", "issuer", c.issuer.clone()),
        InfoRow::text("This Update", "this_update", c.this_update.clone()),
    ];
    if let Some(next) = &c.next_update {
        r.push(InfoRow::text("Next Update", "next_update", next.clone()));
    }
    r.push(InfoRow::count(
        "Revoked",
        "revoked_count",
        c.revoked_count as u64,
    ));
    r.push(InfoRow::text(
        "Signature",
        "signature_algorithm",
        c.signature_algorithm.clone(),
    ));
    r
}

fn key_rows(k: &KeyEntry, kind: &'static str) -> Vec<InfoRow> {
    let mut r = vec![
        InfoRow::json_only("kind", Value::token(kind)),
        InfoRow::muted("Label", "label", k.label.clone()),
        InfoRow::print_only("Type", Value::text(key_type_label(&k.key_type))),
        InfoRow::json_only("key_type", Value::token(key_type_token(&k.key_type))),
    ];
    if let KeyType::Ec(curve) = &k.key_type
        && !curve.is_empty()
    {
        r.push(InfoRow::json_text("curve", curve.clone()));
    }
    if let Some(bits) = k.key_size_bits {
        r.push(InfoRow::new(
            "Bits",
            "key_size_bits",
            Value::int_plain(bits as i64),
        ));
    }
    r
}

fn ssh_rows(k: &SshPubKeyEntry) -> Vec<InfoRow> {
    let mut r = vec![
        InfoRow::json_only("kind", Value::token("ssh_public_key")),
        InfoRow::text("Algorithm", "algorithm", k.algorithm.clone()),
    ];
    if let Some(bits) = k.bits {
        r.push(InfoRow::new("Bits", "bits", Value::int_plain(bits as i64)));
    }
    if !k.comment.is_empty() {
        r.push(InfoRow::muted("Comment", "comment", k.comment.clone()));
    }
    r.push(InfoRow::muted(
        "SHA-256",
        "fingerprint_sha256",
        k.fingerprint_sha256.clone(),
    ));
    r
}

fn jwk_rows(k: &JwkEntry) -> Vec<InfoRow> {
    let type_label = match &k.crv {
        Some(crv) if !crv.is_empty() => format!("{} ({crv})", k.kty),
        _ => k.kty.clone(),
    };
    let mut r = vec![
        InfoRow::json_only("kind", Value::token("json_web_key")),
        InfoRow::print_only("Type", Value::text(type_label)),
        InfoRow::json_text("kty", k.kty.clone()),
    ];
    if let Some(crv) = &k.crv {
        r.push(InfoRow::json_text("crv", crv.clone()));
    }
    if let Some(bits) = k.key_size_bits {
        r.push(InfoRow::new(
            "Bits",
            "key_size_bits",
            Value::int_plain(bits as i64),
        ));
    }
    if let Some(alg) = &k.alg {
        r.push(InfoRow::text("Algorithm", "alg", alg.clone()));
    }
    if let Some(use_) = &k.use_ {
        r.push(InfoRow::text("Use", "use", use_.clone()));
    }
    if !k.key_ops.is_empty() {
        r.push(InfoRow::new(
            "Key Ops",
            "key_ops",
            Value::split(k.key_ops.join(", "), Role::Value, json!(k.key_ops)),
        ));
    }
    if let Some(kid) = &k.kid {
        r.push(InfoRow::muted("Key ID", "kid", kid.clone()));
    }
    if let Some(tp) = &k.thumbprint {
        r.push(InfoRow::muted("Thumbprint", "thumbprint", tp.clone()));
    }
    r
}

fn unknown_rows(u: &UnknownEntry) -> Vec<InfoRow> {
    vec![
        InfoRow::json_only("kind", Value::token("unknown")),
        InfoRow::muted("Label", "label", u.label.clone()),
        InfoRow::count("DER Bytes", "der_bytes", u.der_bytes as u64),
    ]
}

/// A `Public Key` print label: `alg (N bit)`, or bare `alg` when bits unknown.
fn key_algo_label(algorithm: &str, bits: Option<usize>) -> String {
    match bits {
        Some(b) => format!("{algorithm} ({b} bit)"),
        None => algorithm.to_string(),
    }
}

/// A list field: print the comma-joined names, serialize the array, omit both
/// when empty.
fn push_list_row(
    rows: &mut Vec<InfoRow>,
    label: &'static str,
    key: &'static str,
    names: &[String],
) {
    if names.is_empty() {
        return;
    }
    rows.push(InfoRow::new(
        label,
        key,
        Value::split(names.join(", "), Role::Value, json!(names)),
    ));
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

fn key_type_token(kt: &KeyType) -> &'static str {
    match kt {
        KeyType::Rsa => "rsa",
        KeyType::Ec(_) => "ec",
        KeyType::Ed25519 => "ed25519",
        KeyType::Dsa => "dsa",
        KeyType::Other => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cert() -> CertificateEntry {
        CertificateEntry {
            label: "CERTIFICATE".to_string(),
            subject: "CN=example.com".to_string(),
            issuer: "CN=Test CA".to_string(),
            serial_hex: "01:23:AB".to_string(),
            not_before: "2026-01-15T00:00:00Z".to_string(),
            not_after: "2027-01-15T00:00:00Z".to_string(),
            days_remaining: Some(222),
            san_dns: vec!["example.com".to_string(), "www.example.com".to_string()],
            san_ip: Vec::new(),
            san_email: Vec::new(),
            san_uri: Vec::new(),
            key_algorithm: "RSA".to_string(),
            key_size_bits: Some(2048),
            signature_algorithm: "SHA256-RSA".to_string(),
            fingerprint_sha1: "AB:CD".to_string(),
            fingerprint_sha256: "EF:01".to_string(),
            is_ca: true,
            self_signed: false,
            version: 3,
            key_usages: vec!["digitalSignature".to_string()],
            extended_key_usages: Vec::new(),
        }
    }

    /// The JSON object for a certificate entry: keys present, the print≠json
    /// divergences resolved to the machine form, print-only rows absent.
    #[test]
    fn certificate_json_shape() {
        let c = sample_cert();
        let obj = serde_json::Value::Object(rows_to_json(&entry_rows(&CertEntry::Certificate(
            Box::new(c),
        ))));

        assert_eq!(obj["kind"], json!("certificate"));
        assert_eq!(obj["subject"], json!("CN=example.com"));
        assert_eq!(obj["version"], json!(3));
        // Validity split: the print `Days Left` line has no key; the raw count does.
        assert_eq!(obj["days_remaining"], json!(222));
        assert!(obj.get("Days Left").is_none(), "print label leaked: {obj}");
        // Public Key split: one print row, two flat JSON keys.
        assert_eq!(obj["key_algorithm"], json!("RSA"));
        assert_eq!(obj["key_size_bits"], json!(2048));
        assert!(obj.get("Public Key").is_none(), "print label leaked: {obj}");
        // SAN serializes as an array, not the comma-joined print string.
        assert_eq!(
            obj["san_dns"],
            json!(["example.com", "www.example.com"]),
            "SAN must be an array: {obj}"
        );
        assert_eq!(obj["key_usages"], json!(["digitalSignature"]));
        // Bools are always present; their print rows (`CA`) are JSON-keyless.
        assert_eq!(obj["is_ca"], json!(true));
        assert_eq!(obj["self_signed"], json!(false));
        assert!(obj.get("CA").is_none(), "print label leaked: {obj}");
        assert_eq!(obj["fingerprint_sha256"], json!("EF:01"));
    }

    /// The section frame: `cert` key, `source_label`, the entries array, and
    /// `parse_errors` only when non-empty.
    #[test]
    fn section_frame() {
        let info = CertInfo {
            text: None,
            source_label: "PEM",
            entries: vec![CertEntry::Certificate(Box::new(sample_cert()))],
            parse_errors: Vec::new(),
        };
        let (key, value) = json_section(&info);
        assert_eq!(key, "cert");
        assert_eq!(value["source_label"], json!("PEM"));
        assert_eq!(value["entries"].as_array().unwrap().len(), 1);
        assert!(value.get("parse_errors").is_none());
    }

    /// A private-key entry: `kind`/`key_type` tokens, the human `Type` print
    /// row stays out of JSON, bits surface as a number.
    #[test]
    fn private_key_json_shape() {
        let k = KeyEntry {
            label: "PRIVATE KEY".to_string(),
            key_type: KeyType::Rsa,
            key_size_bits: Some(4096),
        };
        let obj = serde_json::Value::Object(rows_to_json(&entry_rows(&CertEntry::PrivateKey(k))));
        assert_eq!(obj["kind"], json!("private_key"));
        assert_eq!(obj["key_type"], json!("rsa"));
        assert_eq!(obj["key_size_bits"], json!(4096));
        assert!(obj.get("Type").is_none(), "print label leaked: {obj}");
    }
}
