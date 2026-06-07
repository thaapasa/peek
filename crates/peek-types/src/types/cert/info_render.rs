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

use serde_json::json;

use crate::info::{
    InfoNode, InfoRow, Role, Value, paint_count, push_rows, render_info, rows_to_json,
};
use crate::theme::PeekTheme;
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
            nodes.push(InfoNode::Line(String::new()));
            let title = entry_title(entry, i + 1);
            nodes.push(InfoNode::Line(format!(
                "{} {}",
                theme.paint_muted("\u{2500}\u{2500}"),
                theme.paint_heading(&title),
            )));
            // One row list drives the print body here and the JSON in
            // `json_section`; `push_rows` emits the print rows as `Line`s.
            let mut body = Vec::new();
            push_rows(&mut body, &entry_rows(entry), theme);
            nodes.extend(body.into_iter().map(InfoNode::Line));
        }

        for err in &info.parse_errors {
            nodes.push(InfoNode::Line(String::new()));
            nodes.push(InfoNode::Row {
                label: "Parse error".into(),
                value: theme.paint(err, theme.warning),
            });
        }
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
        InfoRow::new("Label", "label", Value::muted(c.label.clone())),
        InfoRow::new("Version", "version", int_row(c.version as i64)),
        InfoRow::new("Subject", "subject", Value::text(c.subject.clone())),
        InfoRow::new("Issuer", "issuer", Value::text(c.issuer.clone())),
        InfoRow::new("Serial", "serial_hex", Value::muted(c.serial_hex.clone())),
        InfoRow::new(
            "Not Before",
            "not_before",
            Value::text(c.not_before.clone()),
        ),
        InfoRow::new("Not After", "not_after", Value::text(c.not_after.clone())),
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
        r.push(InfoRow::json_only("days_remaining", Value::int(days)));
    }
    // One print row "alg (bits bit)"; JSON splits into two flat keys.
    r.push(InfoRow::print_only(
        "Public Key",
        Value::text(key_algo_label(&c.key_algorithm, c.key_size_bits)),
    ));
    r.push(InfoRow::json_only(
        "key_algorithm",
        Value::text(c.key_algorithm.clone()),
    ));
    if let Some(b) = c.key_size_bits {
        r.push(InfoRow::json_only("key_size_bits", Value::int(b as i64)));
    }
    r.push(InfoRow::new(
        "Signature",
        "signature_algorithm",
        Value::text(c.signature_algorithm.clone()),
    ));
    push_list_row(&mut r, "SAN DNS", "san_dns", &c.san_dns);
    push_list_row(&mut r, "SAN IP", "san_ip", &c.san_ip);
    push_list_row(&mut r, "SAN Email", "san_email", &c.san_email);
    push_list_row(&mut r, "SAN URI", "san_uri", &c.san_uri);
    // JSON keeps the bool always; print shows the row only when true.
    r.push(InfoRow::json_only("is_ca", Value::bool(c.is_ca)));
    if c.is_ca {
        r.push(InfoRow::print_only("CA", Value::text("yes")));
    }
    r.push(InfoRow::json_only(
        "self_signed",
        Value::bool(c.self_signed),
    ));
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
    r.push(InfoRow::new(
        "SHA-1",
        "fingerprint_sha1",
        Value::muted(c.fingerprint_sha1.clone()),
    ));
    r.push(InfoRow::new(
        "SHA-256",
        "fingerprint_sha256",
        Value::muted(c.fingerprint_sha256.clone()),
    ));
    r
}

fn csr_rows(c: &CsrEntry) -> Vec<InfoRow> {
    let mut r = vec![
        InfoRow::json_only("kind", Value::token("csr")),
        InfoRow::new("Label", "label", Value::muted(c.label.clone())),
        InfoRow::new("Subject", "subject", Value::text(c.subject.clone())),
        InfoRow::print_only(
            "Public Key",
            Value::text(key_algo_label(&c.key_algorithm, c.key_size_bits)),
        ),
        InfoRow::json_only("key_algorithm", Value::text(c.key_algorithm.clone())),
    ];
    if let Some(b) = c.key_size_bits {
        r.push(InfoRow::json_only("key_size_bits", Value::int(b as i64)));
    }
    r.push(InfoRow::new(
        "Signature",
        "signature_algorithm",
        Value::text(c.signature_algorithm.clone()),
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
        InfoRow::new("Label", "label", Value::muted(c.label.clone())),
        InfoRow::new("Issuer", "issuer", Value::text(c.issuer.clone())),
        InfoRow::new(
            "This Update",
            "this_update",
            Value::text(c.this_update.clone()),
        ),
    ];
    if let Some(next) = &c.next_update {
        r.push(InfoRow::new(
            "Next Update",
            "next_update",
            Value::text(next.clone()),
        ));
    }
    r.push(InfoRow::new(
        "Revoked",
        "revoked_count",
        Value::count(c.revoked_count as u64),
    ));
    r.push(InfoRow::new(
        "Signature",
        "signature_algorithm",
        Value::text(c.signature_algorithm.clone()),
    ));
    r
}

fn key_rows(k: &KeyEntry, kind: &'static str) -> Vec<InfoRow> {
    let mut r = vec![
        InfoRow::json_only("kind", Value::token(kind)),
        InfoRow::new("Label", "label", Value::muted(k.label.clone())),
        InfoRow::print_only("Type", Value::text(key_type_label(&k.key_type))),
        InfoRow::json_only("key_type", Value::token(key_type_token(&k.key_type))),
    ];
    if let KeyType::Ec(curve) = &k.key_type
        && !curve.is_empty()
    {
        r.push(InfoRow::json_only("curve", Value::text(curve.clone())));
    }
    if let Some(bits) = k.key_size_bits {
        r.push(InfoRow::new("Bits", "key_size_bits", int_row(bits as i64)));
    }
    r
}

fn ssh_rows(k: &SshPubKeyEntry) -> Vec<InfoRow> {
    let mut r = vec![
        InfoRow::json_only("kind", Value::token("ssh_public_key")),
        InfoRow::new("Algorithm", "algorithm", Value::text(k.algorithm.clone())),
    ];
    if let Some(bits) = k.bits {
        r.push(InfoRow::new("Bits", "bits", int_row(bits as i64)));
    }
    if !k.comment.is_empty() {
        r.push(InfoRow::new(
            "Comment",
            "comment",
            Value::muted(k.comment.clone()),
        ));
    }
    r.push(InfoRow::new(
        "SHA-256",
        "fingerprint_sha256",
        Value::muted(k.fingerprint_sha256.clone()),
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
        InfoRow::json_only("kty", Value::text(k.kty.clone())),
    ];
    if let Some(crv) = &k.crv {
        r.push(InfoRow::json_only("crv", Value::text(crv.clone())));
    }
    if let Some(bits) = k.key_size_bits {
        r.push(InfoRow::new("Bits", "key_size_bits", int_row(bits as i64)));
    }
    if let Some(alg) = &k.alg {
        r.push(InfoRow::new("Algorithm", "alg", Value::text(alg.clone())));
    }
    if let Some(use_) = &k.use_ {
        r.push(InfoRow::new("Use", "use", Value::text(use_.clone())));
    }
    if !k.key_ops.is_empty() {
        r.push(InfoRow::new(
            "Key Ops",
            "key_ops",
            Value::split(k.key_ops.join(", "), Role::Value, json!(k.key_ops)),
        ));
    }
    if let Some(kid) = &k.kid {
        r.push(InfoRow::new("Key ID", "kid", Value::muted(kid.clone())));
    }
    if let Some(tp) = &k.thumbprint {
        r.push(InfoRow::new(
            "Thumbprint",
            "thumbprint",
            Value::muted(tp.clone()),
        ));
    }
    r
}

fn unknown_rows(u: &UnknownEntry) -> Vec<InfoRow> {
    vec![
        InfoRow::json_only("kind", Value::token("unknown")),
        InfoRow::new("Label", "label", Value::muted(u.label.clone())),
        InfoRow::new("DER Bytes", "der_bytes", Value::count(u.der_bytes as u64)),
    ]
}

/// A `Public Key` print label: `alg (N bit)`, or bare `alg` when bits unknown.
fn key_algo_label(algorithm: &str, bits: Option<usize>) -> String {
    match bits {
        Some(b) => format!("{algorithm} ({b} bit)"),
        None => algorithm.to_string(),
    }
}

/// A raw-printed integer (no thousands separator) that serializes as a number —
/// e.g. a version or bit size. Distinct from [`Value::count`], whose print form
/// is grouped and colour-graded.
fn int_row(n: i64) -> Value {
    Value::split(n.to_string(), Role::Value, json!(n))
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
