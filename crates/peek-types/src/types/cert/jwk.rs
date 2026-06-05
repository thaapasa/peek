//! JSON Web Key parsing (RFC 7517 / 7518) + thumbprint (RFC 7638).
//!
//! A JWK is a JSON object describing one key; a JWK Set wraps many under a
//! `keys` array. The structured viewer already pretty-prints the JSON —
//! this adds the decoded sidecar: normalised type / curve / size plus the
//! canonical thumbprint, the fields you actually reach for when eyeballing
//! a key set. Rides the existing `serde_json`; base64url decode/encode
//! comes from the shared `crate::base64`.

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::base64;
use crate::types::cert::info::JwkEntry;

/// Parse a JWK or JWK Set `Value` into entries. A `keys` array → one entry
/// per member; a bare object with a `kty` → a single entry. Non-object
/// members are skipped.
pub fn parse(value: &Value) -> Vec<JwkEntry> {
    if let Some(keys) = value.get("keys").and_then(Value::as_array) {
        return keys.iter().filter_map(parse_one).collect();
    }
    parse_one(value).into_iter().collect()
}

fn parse_one(value: &Value) -> Option<JwkEntry> {
    let kty = value.get("kty").and_then(Value::as_str)?.to_string();
    let str_field = |k: &str| value.get(k).and_then(Value::as_str).map(str::to_string);
    let crv = str_field("crv");
    Some(JwkEntry {
        key_size_bits: key_size_bits(&kty, crv.as_deref(), value),
        thumbprint: thumbprint(&kty, value),
        key_ops: value
            .get("key_ops")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        alg: str_field("alg"),
        use_: str_field("use"),
        kid: str_field("kid"),
        crv,
        kty,
    })
}

/// Best-effort key size. RSA → modulus bit length from `n`; EC / OKP →
/// curve bits; `oct` → bit length of the decoded secret `k`.
fn key_size_bits(kty: &str, crv: Option<&str>, value: &Value) -> Option<usize> {
    match kty {
        "RSA" => {
            let n = value.get("n").and_then(Value::as_str)?;
            Some(modulus_bits(&base64::decode(n)?))
        }
        "EC" | "OKP" => crv.and_then(curve_bits),
        "oct" => {
            let k = value.get("k").and_then(Value::as_str)?;
            Some(base64::decode(k)?.len() * 8)
        }
        _ => None,
    }
}

/// Bit length of a big-endian unsigned integer (the RSA modulus): drop
/// leading zero bytes, then count bits in the top non-zero byte.
fn modulus_bits(bytes: &[u8]) -> usize {
    let first = bytes.iter().position(|&b| b != 0);
    match first {
        Some(i) => (bytes.len() - i - 1) * 8 + (8 - bytes[i].leading_zeros() as usize),
        None => 0,
    }
}

fn curve_bits(crv: &str) -> Option<usize> {
    Some(match crv {
        "P-256" | "secp256k1" => 256,
        "P-384" => 384,
        "P-521" => 521,
        "Ed25519" | "X25519" => 256,
        "Ed448" => 448,
        "X448" => 448,
        _ => return None,
    })
}

/// RFC 7638 thumbprint: SHA-256 over the canonical JSON of the key's
/// required members (lexicographic order, no whitespace), base64url
/// without padding. `None` when a required member is missing.
fn thumbprint(kty: &str, value: &Value) -> Option<String> {
    // Required members per key type, already in lexicographic order.
    let members: &[&str] = match kty {
        "RSA" => &["e", "kty", "n"],
        "EC" => &["crv", "kty", "x", "y"],
        "oct" => &["k", "kty"],
        "OKP" => &["crv", "kty", "x"],
        _ => return None,
    };
    let mut canonical = String::from("{");
    for (i, &m) in members.iter().enumerate() {
        let v = value.get(m).and_then(Value::as_str)?;
        if i > 0 {
            canonical.push(',');
        }
        // Members are short JSON strings; serde_json renders the value
        // with correct escaping (key names here never need any).
        canonical.push_str(&format!("{}:{}", json_string(m), json_string(v)));
    }
    canonical.push('}');

    let digest = Sha256::digest(canonical.as_bytes());
    Some(format!("SHA-256:{}", base64::encode_url(&digest)))
}

/// Render a string as a JSON string literal (quoted + escaped).
fn json_string(s: &str) -> String {
    Value::String(s.to_string()).to_string()
}
