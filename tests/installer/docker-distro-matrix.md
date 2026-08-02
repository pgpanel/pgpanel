# Distro matrix (manual / CI)

| Distro | Package manager | Installer support | CI status |
|--------|-----------------|-------------------|-----------|
| Ubuntu 22.04 | apt | primary | recommended |
| Ubuntu 24.04 | apt | primary | recommended |
| Debian 12 | apt | primary | recommended |
| Debian 11 | apt | supported | manual |
| Rocky Linux 9 | dnf | supported | manual |
| AlmaLinux 9 | dnf | supported | manual |
| RHEL 9 | dnf | supported | manual |
| Fedora 39+ | dnf | supported | manual |
| Amazon Linux 2023 | dnf | supported | manual |
| openSUSE Leap 15 | zypper | best-effort | manual |
| Arch Linux | pacman | best-effort | manual |
| Alpine | apk | best-effort | limited |

## Scenario checklist

- [ ] Fresh install (interactive)
- [ ] Re-run installer (idempotent)
- [ ] Update mode preserves `.env`
- [ ] Repair recreates networks
- [ ] Bad domain (warn, continue)
- [ ] Port 80/443 occupied (warn)
- [ ] Docker missing → install
- [ ] Bad git repo URL
- [ ] Bad S3 credentials → menu (retry/skip/abort)
- [ ] Ctrl+C mid-install
- [ ] Existing `.env` not regenerated
- [ ] Existing volumes retained on update
- [ ] Uninstall keep volumes
- [ ] Full uninstall with `DELETE ALL DATA`

## Suggested container smoke (privileged)

```bash
# Ubuntu 24.04
docker run --rm -it --privileged -v /var/run/docker.sock:/var/run/docker.sock \
  -v "$PWD":/src -w /src ubuntu:24.04 bash -lc \
  'apt-get update && apt-get install -y curl ca-certificates git openssl bats shellcheck && bash tests/installer/shellcheck.sh && bats tests/installer'
```

Note: full Docker-in-Docker install tests require privileged hosts and are not part of the default unit suite.
