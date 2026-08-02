# Gyors start

**Fejlesztő:** Dezső Benedek Péter  

## Telepítés

```bash
sudo apt-get update && sudo apt-get install -y curl && \
curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash
```

Nincs Git repo megadás. Hivatalos forrás: `https://github.com/pgpanel/pgpanel.git`.

## Admin

| Mező | Érték |
|------|--------|
| Felhasználónév | `admin` (telepítéskor módosítható) |
| Jelszó | Generált vagy kézzel megadott — **nincs** fix gyári jelszó |
| Első belépés | `https://DOMAIN/setup` |

Jelszó egyszer a telepítő kimenetén és/vagy: `/etc/pgpanel/.admin-password-ONCE`  
Bootstrap token: `/opt/pgpanel/.env` → `PGPANEL_BOOTSTRAP_TOKEN`

## Frissítés

```bash
sudo pgpanel version   # helyi vs remote
sudo pgpanel update    # adatok megmaradnak
```

## Build a VPS-en?

**Nem kötelező.** Alapból a panel image a GHCR-ről jön (`ghcr.io/pgpanel/pgpanel:<verzió>`). A szerver csak Docker image-eket húz le.

Helyi build: `sudo bash /opt/pgpanel/deploy/install.sh --build-local` (lassú, sok RAM).
