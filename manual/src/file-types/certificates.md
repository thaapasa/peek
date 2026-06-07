# Certificates and keys

Certificate and key files open with a rich **Info** sidecar that decodes the cryptographic
material: X.509 certificates, certificate signing requests (CSRs), certificate revocation lists
(CRLs), private keys (RSA / EC / Ed25519 / DSA / PKCS#8), public keys, OpenSSH public-key files
(`.pub`), and JSON Web Keys. The source view depends on the container: PEM shows its text, a JWK
shows the pretty-printed JSON, and raw DER is binary so it gets the Info sidecar and the
[hex dump](./binary.md) only.

A PEM file may contain many entries — a fullchain bundle, for example, holds one or more
certificates plus an intermediate. A JWK Set holds one key per `keys` member; a DER file is a
single entry. Every entry is decoded and rendered as its own block in the Info section.

## Detection

- **By extension** — `.pem` / `.csr` / `.crl` / `.key` / `.p7b` / `.p7c` / `.pub` (PEM), `.der`
  (DER), `.jwk` / `.jwks` (JWK).
- **By content** — anything that starts with `-----BEGIN ` (any label, PEM), an OpenSSH algorithm
  prefix (`ssh-rsa`, `ssh-ed25519`, `ecdsa-sha2-…`, including the FIDO/U2F `sk-*` variants), a
  binary DER certificate, or a JSON object whose `kty` is a known key type.

`.crt` and `.cer` are intentionally *not* routed by extension because they carry *either*
encoding. A PEM-encoded one is picked up by the `-----BEGIN ` sniff; a DER-encoded one is
recognised when its bytes decode as an X.509 certificate. (DER carries no label, so peek tries
certificate, then CRL, CSR, and key in turn.) A JWK saved as `.json` keeps the generic JSON view —
the JWK content sniff only applies to `.jwk` / `.jwks`, stdin, and extension-less files.

## What you see

Per entry, the Info section surfaces:

- **X.509 certificate** — subject, issuer, serial, NotBefore / NotAfter, days remaining (painted
  as a warning when ≤ 30 days; expired certs show as `expired N days ago`), public-key algorithm
  + bits, signature algorithm, Subject Alternative Names (DNS / IP / email / URI), CA flag,
  self-signed flag, key usage, extended key usage, SHA-1 and SHA-256 fingerprints.
- **CSR (PKCS#10)** — subject, requested SANs, public-key algorithm + bits, signature algorithm.
- **CRL** — issuer, This Update / Next Update, revoked entry count, signature algorithm.
- **Private key** — label, key type (RSA / EC + curve / Ed25519 / DSA / opaque), bit size when
  derivable. Encrypted (`ENCRYPTED PRIVATE KEY`) and `OPENSSH PRIVATE KEY` blocks show
  structural info only — no password prompt.
- **Public key** — label, key type, bit size (parsed from the SPKI envelope).
- **SSH public key** — algorithm, bits, comment, SHA-256 fingerprint matching `ssh-keygen -l`.
- **JSON Web Key** — type (RSA / EC / oct / OKP) and curve, bit size, algorithm, use, key
  operations, key ID, and the RFC 7638 thumbprint.

Decode failures don't suppress the rest of the section. A malformed block surfaces as a per-entry
**Parse error** row so one bad PEM in a chain doesn't hide the others. Unrecognised PEM labels
show as `Unknown` entries with the original label and the decoded DER body size.

## Source view

The default view is the PEM text — exactly what the file contains. Tab or `i` jumps to Info.
Hex (`x`) is still available; for cert files it's rarely what you want, but it's there.

## Limitations

PKCS#12 / PFX containers and encrypted PKCS#8 (password prompt) are not yet decoded. They're
tracked as follow-up work.
