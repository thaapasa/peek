//! Certificate / key info. Decodes X.509 certificates, CSRs, CRLs,
//! private/public keys, and OpenSSH public keys from PEM text; the same
//! X.509 decoders run over raw DER. JSON Web Keys (`.jwk` / `.jwks`) add
//! a normalised key sidecar over the pretty-printed JSON. The value-add
//! across all forms is the parsed Info section.

pub mod compose;
pub mod info;
pub mod info_gather;
pub mod info_render;
pub mod jwk;
