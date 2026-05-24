# Certificates and keys

PEM-encoded certificate and key files open in the source viewer with a rich **Info** sidecar
that decodes every PEM block: X.509 certificates, certificate signing requests (CSRs),
certificate revocation lists (CRLs), private keys (RSA / EC / Ed25519 / DSA / PKCS#8), public
keys, and OpenSSH public-key files (`.pub`).

A single file may contain many entries — a fullchain bundle, for example, holds one or more
certificates plus an intermediate. Every entry is decoded and rendered as its own block in the
Info section.

## Detection

- **By extension** — `.pem` / `.csr` / `.crl` / `.key` / `.p7b` / `.p7c` / `.pub`.
- **By content** — anything that starts with `-----BEGIN ` (any label), or an OpenSSH algorithm
  prefix (`ssh-rsa`, `ssh-ed25519`, `ecdsa-sha2-…`, including the FIDO/U2F `sk-*` variants).

`.crt` and `.cer` are intentionally *not* routed by extension because they routinely carry raw
DER as well as PEM. A PEM-encoded `.crt` is picked up by the `-----BEGIN ` content sniff; a
DER-encoded `.crt` falls through to the hex viewer, which is more useful than a mojibake source
dump.

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

Decode failures don't suppress the rest of the section. A malformed block surfaces as a per-entry
**Parse error** row so one bad PEM in a chain doesn't hide the others. Unrecognised PEM labels
show as `Unknown` entries with the original label and the decoded DER body size.

## Source view

The default view is the PEM text — exactly what the file contains. Tab or `i` jumps to Info.
Hex (`x`) is still available; for cert files it's rarely what you want, but it's there.

## Limitations

DER-encoded files (`.der`, DER-form `.crt` / `.cer`), PKCS#12 / PFX containers, encrypted PKCS#8
with a password prompt, and JWK / JWKS are not yet decoded. They're tracked as follow-up work.
