#!/usr/bin/env bash
# Regenerate the PEM / SSH-pubkey test fixtures in `test-data/`.
#
# Run once to produce the checked-in samples; not part of the build.
# Requires `openssl` and `ssh-keygen` on $PATH. Fixtures use long
# validity windows (10 years) so they don't expire mid-decade and
# break manual inspection of the "days remaining" field.
#
# Subject lines deliberately use generic strings — these are decoys
# only, never wired to anything real.

set -euo pipefail

cd "$(dirname "$0")/../test-data"

# Long-lived self-signed RSA cert (CN=peek-demo.example.com), with
# DNS / IP / email SANs and the usual TLS server-auth EKU. Single
# `openssl req -x509` invocation keeps key + cert in one command.
openssl req -x509 -newkey rsa:2048 -nodes \
    -keyout rsa-key.pem \
    -out cert-rsa.pem \
    -days 3650 \
    -subj "/CN=peek-demo.example.com/O=Peek Demo/C=FI" \
    -addext "subjectAltName=DNS:peek-demo.example.com,DNS:*.peek-demo.example.com,IP:192.0.2.10,email:demo@peek-demo.example.com" \
    -addext "keyUsage=digitalSignature,keyEncipherment" \
    -addext "extendedKeyUsage=serverAuth,clientAuth" \
    >/dev/null 2>&1

# EC P-256 self-signed cert with the matching key.
openssl ecparam -name prime256v1 -genkey -noout -out ec-key.pem
openssl req -new -x509 -key ec-key.pem -out cert-ec.pem \
    -days 3650 \
    -subj "/CN=peek-ec.example.com/O=Peek Demo/C=FI" \
    -addext "subjectAltName=DNS:peek-ec.example.com" \
    -addext "extendedKeyUsage=serverAuth" \
    >/dev/null 2>&1

# Ed25519 key + self-signed cert.
openssl genpkey -algorithm Ed25519 -out ed25519-key.pem >/dev/null 2>&1
openssl req -new -x509 -key ed25519-key.pem -out cert-ed25519.pem \
    -days 3650 \
    -subj "/CN=peek-ed25519.example.com/O=Peek Demo/C=FI" \
    -addext "subjectAltName=DNS:peek-ed25519.example.com" \
    >/dev/null 2>&1

# CSR (PKCS#10) signed by the RSA key.
openssl req -new -key rsa-key.pem -out request.csr \
    -subj "/CN=request.example.com/O=Peek Demo/C=FI" \
    -addext "subjectAltName=DNS:request.example.com,DNS:alt.example.com" \
    >/dev/null 2>&1

# CRL signed by the RSA self-signed cert. The CA-config dance is the
# minimum openssl wants to issue a CRL — temp dir, blank serial, blank
# db. Cleaned up after the CRL lands in place.
crl_tmp=$(mktemp -d)
trap 'rm -rf "$crl_tmp"' EXIT
mkdir -p "$crl_tmp/newcerts"
: > "$crl_tmp/index.txt"
: > "$crl_tmp/index.txt.attr"
echo "01" > "$crl_tmp/crlnumber"

cat > "$crl_tmp/openssl.cnf" <<EOF
[ ca ]
default_ca = peek_demo_ca

[ peek_demo_ca ]
dir              = $crl_tmp
database         = \$dir/index.txt
new_certs_dir    = \$dir/newcerts
certificate      = $(pwd)/cert-rsa.pem
private_key      = $(pwd)/rsa-key.pem
serial           = \$dir/serial
crlnumber        = \$dir/crlnumber
default_md       = sha256
default_crl_days = 30
policy           = peek_demo_policy

[ peek_demo_policy ]
commonName = supplied
EOF

openssl ca -config "$crl_tmp/openssl.cnf" -gencrl -out demo.crl >/dev/null 2>&1

# OpenSSH public keys — RSA + Ed25519 + ECDSA. `-q -N ''` makes the
# generation non-interactive with no passphrase; the matching private
# key lands beside each `.pub` and isn't needed by peek (we already
# have private keys above). Delete the private halves to keep the
# fixture surface focused on the public-key viewer.
ssh_tmp=$(mktemp -d)
ssh-keygen -t rsa -b 2048 -N '' -C 'peek-demo@example.com' -f "$ssh_tmp/id_rsa" -q
ssh-keygen -t ed25519 -N '' -C 'peek-ed@example.com' -f "$ssh_tmp/id_ed25519" -q
ssh-keygen -t ecdsa -b 256 -N '' -C 'peek-ecdsa@example.com' -f "$ssh_tmp/id_ecdsa" -q
cp "$ssh_tmp/id_rsa.pub" ssh-rsa.pub
cp "$ssh_tmp/id_ed25519.pub" ssh-ed25519.pub
cp "$ssh_tmp/id_ecdsa.pub" ssh-ecdsa.pub
rm -rf "$ssh_tmp"

# Multi-block "fullchain" sample: the EC cert followed by the RSA
# cert in one PEM. Mimics what a TLS server typically serves.
cat cert-ec.pem cert-rsa.pem > fullchain.pem

echo "fixtures regenerated:"
ls -1 cert-rsa.pem cert-ec.pem cert-ed25519.pem \
    rsa-key.pem ec-key.pem ed25519-key.pem \
    request.csr demo.crl \
    ssh-rsa.pub ssh-ed25519.pub ssh-ecdsa.pub \
    fullchain.pem
