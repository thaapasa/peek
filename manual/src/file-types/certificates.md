# Certificates and keys

Certificate and key files open with a detailed **Info** sidecar that decodes the cryptographic
material. peek decodes these kinds of files:

- X.509 certificates
- Certificate signing requests (CSRs)
- Certificate revocation lists (CRLs)
- Private keys, such as RSA, EC, Ed25519, DSA, and PKCS#8 keys
- Public keys
- OpenSSH public-key files (`.pub`)
- JSON Web Keys (JWK)

The source view depends on the container format. For a PEM file, it shows the text. For a JWK, it
shows the pretty-printed JSON. Raw DER is binary, so a DER file gets only the Info sidecar and the
[hex dump](./binary.md).

A PEM file can contain many entries. For example, a full-chain bundle holds one or more certificates
and an intermediate certificate. A JWK Set holds one key for each `keys` member. A DER file holds a
single entry. peek decodes every entry and shows it as its own block in the Info section.

## Detection

peek detects these files in two ways:

- **By extension**:
  - PEM: `.pem`, `.csr`, `.crl`, `.key`, `.p7b`, `.p7c`, and `.pub`
  - DER: `.der`
  - JWK: `.jwk` and `.jwks`
- **By content**:
  - PEM: any file that starts with `-----BEGIN `, with any label
  - OpenSSH: a file that starts with an algorithm prefix, such as `ssh-rsa`, `ssh-ed25519`, or
    `ecdsa-sha2-…`, including the FIDO U2F `sk-*` variants
  - DER: a binary DER certificate
  - JWK: a JSON object whose `kty` is a known key type

peek doesn't detect `.crt` and `.cer` files by extension, because these files can use either
encoding. peek picks up a PEM-encoded file by its `-----BEGIN ` prefix. It recognizes a DER-encoded
file if the bytes decode as an X.509 certificate. DER carries no label, so peek tries to decode the
file as a certificate, a CRL, a CSR, and a key, in that order.

If you save a JWK as `.json`, it keeps the generic JSON view. peek checks the content for a JWK only
in `.jwk` and `.jwks` files, standard input (stdin), and files without an extension.

## What you see

For each entry, the Info section shows these details:

- **X.509 certificate**:
  - Subject, issuer, and serial number
  - The validity period, NotBefore to NotAfter, and the days remaining. If 30 days or fewer remain,
    peek paints the count as a warning. An expired certificate shows as `expired N days ago`.
  - Public-key algorithm and key size in bits
  - Signature algorithm
  - Subject Alternative Names (SANs), which can be DNS names, IP addresses, email addresses, and
    URIs
  - CA flag and self-signed flag
  - Key usage and extended key usage
  - SHA-1 and SHA-256 fingerprints
- **CSR (PKCS#10)**: subject, requested SANs, public-key algorithm and bits, and signature
  algorithm.
- **CRL**: issuer, This Update, Next Update, the number of revoked entries, and the signature
  algorithm.
- **Private key**: label, key type, and the bit size if peek can work it out. The key type is RSA,
  EC with its curve, Ed25519, DSA, or opaque. For encrypted blocks (`ENCRYPTED PRIVATE KEY`) and
  `OPENSSH PRIVATE KEY` blocks, peek shows only the structure. It doesn't ask for a password.
- **Public key**: label, key type, and bit size. peek reads these from the Subject Public Key Info
  (SPKI) envelope.
- **SSH public key**: algorithm, bits, comment, and the SHA-256 fingerprint. The fingerprint matches
  the output of `ssh-keygen -l`.
- **JSON Web Key**: key type (RSA, EC, oct, or OKP) and curve, bit size, algorithm, use, key
  operations, key ID, and the RFC 7638 thumbprint.

If an entry fails to decode, peek still shows the rest of the section. The broken entry gets a
**Parse error** row, so one bad PEM block in a chain doesn't hide the others. If peek doesn't
recognize a PEM label, it shows the entry as `Unknown`. The entry shows the original label and the
size of the decoded DER body.

## Source view

For a PEM file, the default view is the PEM text, exactly as the file contains it. To jump to the
Info view, press Tab or `i`. The hex view (`x`) is still available, though you rarely need it for a
certificate file.

## Limitations

peek doesn't decode PKCS#12 and PFX containers yet. It also doesn't decode encrypted PKCS#8 keys,
which need a password prompt. Both are planned work.
