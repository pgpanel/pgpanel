# Integration tests

## Unit tests (always run)

```bash
cargo test --workspace
```

Covered today:

- Identifier validation / slugify
- Argon2 + AES-GCM crypto
- SQL console allow/deny parser rules
- Databasus mock adapter

## Docker / PostgreSQL integration (optional)

These require a running Docker daemon and pull `postgres:17`.

Suggested cases (implement or extend as CI allows):

| Case | Expectation |
|------|-------------|
| Docker daemon down | create cluster → operation fails; panel stays up |
| Port conflict | public port already bound → Failed status, volume kept |
| Bad image | rejected at validation (not allowlisted) |
| Disk full | Docker error; volume not auto-deleted |
| Interrupted create | worker restart re-queues or fails stale running jobs |
| Databasus down | cluster `healthy_with_backup_warning` |
| Role ok, DB fails | role remains; job failed; retry safe |
| Duplicate create (idempotency key) | single operation |
| Delete protection | permanent delete rejected |
| Volume delete | only with `confirm_volume_delete` |

Run manually against a lab host after `install.sh`.

## Auth tests

```bash
# start panel with test env, then:
curl -s localhost:8080/api/auth/status
curl -s -c jar -X POST localhost:8080/api/auth/bootstrap \
  -H 'content-type: application/json' \
  -d '{"username":"admin","password":"testpassword12345"}'
```

CSRF: mutating calls without `x-csrf-token` must return 403.
