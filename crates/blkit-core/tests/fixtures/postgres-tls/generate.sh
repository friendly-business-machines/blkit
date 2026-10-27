#!/usr/bin/env bash
# Test-only keys, fixed validity windows; never use these credentials outside tests.
set -euo pipefail
cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
openssl req -new -newkey rsa:2048 -nodes -keyout "$tmp/ca.key" -out "$tmp/ca.csr" -subj '/CN=blkit connector test CA'
openssl req -new -newkey rsa:2048 -nodes -keyout server.key -out "$tmp/server.csr" -subj '/CN=localhost'
touch "$tmp/index"
echo 01 > "$tmp/serial"
cat > "$tmp/ca.cnf" <<CONFIG
[ca]
default_ca = test_ca
[test_ca]
database = $tmp/index
serial = $tmp/serial
new_certs_dir = $tmp
private_key = $tmp/ca.key
certificate = $PWD/ca.pem
default_md = sha256
policy = policy
unique_subject = no
[policy]
commonName = supplied
[root]
basicConstraints = critical,CA:true
keyUsage = critical,keyCertSign,cRLSign
subjectKeyIdentifier = hash
[server]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature,keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = DNS:localhost
CONFIG
openssl ca -batch -selfsign -config "$tmp/ca.cnf" -in "$tmp/ca.csr" -out ca.pem -notext -startdate 20200101000000Z -enddate 21200101000000Z -extensions root
openssl ca -batch -config "$tmp/ca.cnf" -in "$tmp/server.csr" -out server.pem -notext -startdate 20200101000000Z -enddate 21200101000000Z -extensions server
openssl ca -batch -config "$tmp/ca.cnf" -in "$tmp/server.csr" -out expired.pem -notext -startdate 20000101000000Z -enddate 20010101000000Z -extensions server
chmod 644 server.key
