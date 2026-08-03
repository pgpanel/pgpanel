# Cloudflare Tunnel

PgPanel listens on **loopback only**. Cloudflare Tunnel (`cloudflared`) provides secure external access without opening firewall ports on the host.

## Architecture

```
Browser → Cloudflare Edge → cloudflared → http://127.0.0.1:8080 (Caddy) → PgPanel
```

Caddy terminates the local reverse proxy and forwards to the active blue/green slot.

## Installer-managed remotely managed tunnel

Create a remotely managed tunnel in the Cloudflare dashboard and configure its public hostname there. Set the service/origin to exactly:

```
http://127.0.0.1:8080
```

Then run the PgPanel installer with the token:

```bash
sudo env PGPANEL_CLOUDFLARE_TUNNEL_TOKEN='<remotely-managed-token>' \
  ./scripts/install.sh --exposure cloudflare
```

The token prompt is silent in interactive runs. Prefer the environment variable over `--cloudflare-token`, because command-line arguments may be visible in shell history or process listings. PgPanel never logs or stores the token. Cloudflare's official `cloudflared service install` command creates the system service and manages what it needs to run the tunnel.

The installer uses Cloudflare's official Ubuntu 24.04 repository:

```text
deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] https://pkg.cloudflare.com/cloudflared any main
```

It does not call Cloudflare APIs or attempt to create DNS or hostname routes.

## Manual installation

If you do not use installer-managed exposure, follow [Cloudflare's official package documentation](https://pkg.cloudflare.com/) and [Tunnel documentation](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/). The official `cloudflared` apt repository uses the distribution-independent `any main` suite shown above.

For a remotely managed tunnel, use the token command shown by the Cloudflare dashboard:

```bash
sudo cloudflared service install '<remotely-managed-token>'
sudo systemctl enable --now cloudflared
```

## PgPanel trusted proxy settings

`--exposure cloudflare` configures trusted loopback proxies and secure cookies in `/etc/pgpanel/pgpanel.toml`:

```toml
[trusted_proxies]
enabled = true
proxies = ["127.0.0.1", "::1"]
prefer_cf_connecting_ip = false

[session]
secure = true
```

If `cloudflared` connects from a non-loopback address, add its source IP/CIDR to `proxies`.

PgPanel deliberately does not trust `CF-Connecting-IP` directly. It accepts the
normal forwarded chain only from the configured loopback proxy, preventing a
client-supplied `CF-Connecting-IP` header from overriding the audit address.

## Service ownership

After successfully installing and activating the service, the installer creates
`/etc/pgpanel/cloudflared-managed`. PgPanel only reinstalls or removes
`cloudflared.service` while this marker exists.

If an unmarked service already exists, `--exposure cloudflare` fails without
altering it. Back up and remove that service yourself before asking PgPanel to
manage a tunnel. Switching to local or public exposure preserves an unmarked
service, but removes a PgPanel-managed service and marker.

## Cloudflare Access (strongly recommended)

Do not rely on PgPanel authentication alone for internet-exposed deployments.

1. Create an Access application for `panel.example.com`
2. Require identity provider + MFA
3. Allow only authorized email domains or groups

Example policy: allow `@yourcompany.com` with one-time PIN or WebAuthn.

## Security notes

- Tunnel target must be `http://127.0.0.1:8080` — **not** the blue/green slots directly
- Do not expose PostgreSQL (5432) through the same tunnel
- PgPanel application updates do not restart `cloudflared`; installer repair runs reinstall/restart it when Cloudflare exposure is selected with a token
- Use Cloudflare WAF rules for additional rate limiting if needed

## Verification

```bash
curl -sI https://panel.example.com/health/live
```

From the host:

```bash
curl -s http://127.0.0.1:8080/health/ready
```

## Troubleshooting

| Symptom | Check |
|---------|-------|
| 502 from Cloudflare | `systemctl status caddy pgpanel-blue` |
| Redirect loop | `session.secure` vs actual scheme |
| Wrong client IP in audit | `trusted_proxies` configuration |
| Tunnel not connecting | `journalctl -u cloudflared -f` |

See [INSTALL.md](../INSTALL.md) and [SECURITY.md](../SECURITY.md).
