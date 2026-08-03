# Cloudflare Tunnel

PgPanel listens on **loopback only**. Cloudflare Tunnel (`cloudflared`) provides secure external access without opening firewall ports on the host.

## Architecture

```
Browser → Cloudflare Edge → cloudflared → http://127.0.0.1:8080 (Caddy) → PgPanel
```

Caddy terminates the local reverse proxy and forwards to the active blue/green slot.

## Install cloudflared

Follow [Cloudflare's documentation](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/) for Ubuntu 24.04.

```bash
# Example: install from Cloudflare package repository
curl -fsSL https://pkg.cloudflare.com/cloudflare-main.gpg | sudo tee /usr/share/keyrings/cloudflare-main.gpg >/dev/null
echo 'deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] https://pkg.cloudflare.com/cloudflared jammy main' | sudo tee /etc/apt/sources.list.d/cloudflared.list
sudo apt-get update && sudo apt-get install -y cloudflared
```

## Tunnel configuration

### config.yml example

```yaml
tunnel: <TUNNEL_UUID>
credentials-file: /etc/cloudflared/<TUNNEL_UUID>.json

ingress:
  - hostname: panel.example.com
    service: http://127.0.0.1:8080
    originRequest:
      noTLSVerify: false
      connectTimeout: 30s
  - service: http_status:404
```

### DNS

Create a CNAME record pointing `panel.example.com` to `<TUNNEL_UUID>.cfargotunnel.com`.

### systemd

```bash
sudo cloudflared service install
sudo systemctl enable --now cloudflared
```

## PgPanel trusted proxy settings

When traffic arrives via `cloudflared` on localhost, enable trusted proxies in `/etc/pgpanel/pgpanel.toml`:

```toml
[trusted_proxies]
enabled = true
proxies = ["127.0.0.1", "::1"]
prefer_cf_connecting_ip = true

[session]
secure = true
```

If `cloudflared` connects from a non-loopback address, add its source IP/CIDR to `proxies`.

## Cloudflare Access (strongly recommended)

Do not rely on PgPanel authentication alone for internet-exposed deployments.

1. Create an Access application for `panel.example.com`
2. Require identity provider + MFA
3. Allow only authorized email domains or groups

Example policy: allow `@yourcompany.com` with one-time PIN or WebAuthn.

## Security notes

- Tunnel target must be `http://127.0.0.1:8080` — **not** the blue/green slots directly
- Do not expose PostgreSQL (5432) through the same tunnel
- PgPanel updates do not restart `cloudflared`
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
