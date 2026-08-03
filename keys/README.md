# Release signing keys

PgPanel release artifacts are verified by `pgpanel-updater` using an Ed25519
public key pinned at `/etc/pgpanel/signing.pub` on installed systems.

The verifier expects:

- **Public key file:** raw 32-byte Ed25519 public key encoded as lowercase hex
  (64 hex chars), not PEM.
- **`SHA256SUMS.sig`:** raw 64-byte PureEdDSA signature (or the same bytes as
  hex). OpenSSL `pkeyutl -sign -rawin` produces the raw form used by CI.

## Generate a signing key pair

Use OpenSSL 3 (available on Ubuntu 24.04 / GitHub `ubuntu-24.04` runners):

```bash
# Private key — keep offline / in CI secrets. NEVER commit.
openssl genpkey -algorithm ED25519 -out signing.key
chmod 600 signing.key

# Public key as raw 32-byte hex (what the updater loads)
openssl pkey -in signing.key -pubout -outform DER \
  | tail -c 32 \
  | xxd -p -c 32 \
  > signing.pub

# Optional: keep a PEM public key for human inspection / openssl tooling
openssl pkey -in signing.key -pubout -out signing.pub.pem
```

`signing.pub` content should look like a single line of 64 hex characters, for
example (placeholder — replace with your real key):

```text
0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
```

See `signing.pub.example` in this directory for the expected format.

### Local signature round-trip (optional)

```bash
# Sign exact SHA256SUMS bytes (same as the release workflow)
openssl pkeyutl -sign -inkey signing.key -rawin \
  -in SHA256SUMS -out SHA256SUMS.sig

# Confirm 64-byte raw signature
wc -c SHA256SUMS.sig
```

## GitHub Actions secret

Store the **private** key in repository secret `PGPANEL_SIGNING_KEY`.

**Required format (pick one):**

1. **Preferred:** full Ed25519 PEM private key text, including
   `-----BEGIN PRIVATE KEY-----` / `-----END PRIVATE KEY-----` lines
   (GitHub secrets support multiline values).
2. **Alternative:** standard base64 encoding of that same PEM file
   (`base64 -w0 signing.key` on Linux).

The release workflow **fails** if `PGPANEL_SIGNING_KEY` is missing or not a
valid Ed25519 PEM. Signing is mandatory for publish; `SHA256SUMS.sig` is always
uploaded as a raw 64-byte file.

Never put the private key in the repository, release archives, logs, or
workflow outputs.

### Install the public key on servers

Copy the hex `signing.pub` to `/etc/pgpanel/signing.pub` during install (or
replace the example file shipped for documentation). The example under
`keys/signing.pub.example` is not a production key.

## Release artifact layout (compatibility notes)

Each platform archive (`pgpanel-linux-amd64.tar.gz` /
`pgpanel-linux-arm64.tar.gz`) is flat-rooted so extraction places files
directly under the release directory:

- `bin/` — `pgpanel-web`, `pgpanel-helper`, `pgpanel-updater`
- `static/`, `templates/`, `migrations/`
- `config/`, `packaging/`, `scripts/`, `keys/`
- `VERSION`, `GIT_TAG`, `GIT_COMMIT`, `BUILD_TIME`

Release assets also include:

- `manifest.json` — Rust `ReleaseManifest` shape:
  `{version, released_at, min_upgrade_version, artifacts, changelog}`
  where `artifacts` maps `linux-amd64` / `linux-arm64` → archive filename
- `SHA256SUMS` — checksums for both archives **and** `manifest.json`
- `SHA256SUMS.sig` — Ed25519 signature over the exact `SHA256SUMS` bytes

Prerelease channel behavior is driven by the GitHub Release `prerelease` flag
(tags containing `-`, e.g. `v1.2.0-rc.1`), not by a field in `manifest.json`.
The updater `stable` channel ignores prereleases; `prerelease` includes them.

## Rotating keys

1. Generate a new key pair.
2. Ship the new hex public key as `/etc/pgpanel/signing.pub` (and update
   `signing.pub.example` / docs for new installs).
3. Update installed systems' public key before requiring signatures from the
   new private key.
4. Rotate the `PGPANEL_SIGNING_KEY` GitHub secret to the new PEM private key.

## Security

- Never commit `signing.key` or any private key material.
- Restrict CI secret access to the release workflow only.
- Prefer hardware-backed or OIDC-based signing for high-assurance production
  releases when available.
