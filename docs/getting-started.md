# Gyors start: admin, Git, egyparancsos telepítés

## 1. Mi a default admin?

| Mező | Érték |
|------|--------|
| **Felhasználónév** | `admin` (telepítő alapértelmezés; átírható) |
| **Jelszó** | **Nincs fix gyári jelszó** — a telepítő generál egyet, vagy te adod meg |
| **Első belépés** | Böngésző: `https://DOMAIN/setup` (bootstrap) |

### Hol találod a jelszót telepítés után?

1. A telepítő **egyszer** kiírja a terminálra (sárga keret).
2. Ideiglenes fájl: `/etc/pgpanel/.admin-password-ONCE` (0600) — **mentsd el, majd töröld**.
3. Bootstrap token (ha kell a setuphoz): szintén a telepítő végén jelenik meg; benne van a `/opt/pgpanel/.env` → `PGPANEL_BOOTSTRAP_TOKEN` mezőben is.

A panel **nem** hoz létre automatikusan usert a Docker-indításkor: az első admin a webes `/setup` oldalon jön létre (username + jelszó + opcionális bootstrap token).

**Nincs** default jelszó mint `admin/admin` — production biztonság miatt.

---

## 2. Hogyan töltsd fel GitHubra?

A projekt mappa jelenleg **nincs** git repo-ként inicializálva. Egyszer:

```bash
cd /path/to/ddb-panel   # vagy pgpanel

git init
git add .
# Ne commitolj .env-et, titkokat, target/, node_modules-t (.gitignore már kizárja)
git status

git commit -m "Initial PgPanel MVP: panel, installer, deploy"

# GitHubon hozz létre üres repót (pl. pgpanel), majd:
git branch -M main
git remote add origin https://github.com/pgpanel/pgpanel.git
git push -u origin main
```

SSH-val:

```bash
git remote add origin git@github.com:pgpanel/pgpanel.git
git push -u origin main
```

### Fontos a one-linerhez

Szerkeszd az `install-pgpanel.sh` tetején:

```bash
readonly DEFAULT_REPO="${PGPANEL_REPO:-https://github.com/pgpanel/pgpanel.git}"
```

cseréld a saját repódra, commitold, pushold. Utána a raw URL működik:

```text
https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh
```

Privát repo: a raw curl nem működik auth nélkül. Használj:

```bash
# gép a VPS-en, clone tokennel, majd helyi install
git clone https://<TOKEN>@github.com/pgpanel/pgpanel.git /opt/pgpanel
sudo bash /opt/pgpanel/deploy/install.sh
```

(A token ne kerüljön shell historyba: `read` + env, vagy SSH deploy key.)

---

## 3. Hogyan töltsd le / telepítsd? (Databasus-stílus)

### Ajánlott egy parancs (Ubuntu/Debian VPS)

```bash
sudo apt-get update && sudo apt-get install -y curl && \
curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash
```

Repo / branch felülírás:

```bash
export PGPANEL_REPO=https://github.com/pgpanel/pgpanel.git
export PGPANEL_REF=main
# opcionális: export PGPANEL_MODE=install   # menü helyett közvetlen mód

sudo apt-get update && sudo apt-get install -y curl && \
curl -sSL "https://raw.githubusercontent.com/pgpanel/pgpanel/${PGPANEL_REF}/install-pgpanel.sh" | sudo bash
```

Egy sorban env-vel:

```bash
sudo apt-get update && sudo apt-get install -y curl && \
PGPANEL_REPO=https://github.com/pgpanel/pgpanel.git \
curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | sudo bash
```

### Mit csinál a one-liner?

1. Root ellenőrzés  
2. `curl` / `git` / `openssl` telepítése ha hiányzik  
3. Repo klónozása → `/opt/pgpanel` (vagy frissítés)  
4. Elindítja a teljes interaktív `deploy/install.sh` menüt  

### Helyi / már klónozott tree

```bash
git clone https://github.com/pgpanel/pgpanel.git
cd pgpanel
sudo bash install.sh
# vagy
sudo bash deploy/install.sh
```

### Telepítés után

```bash
pgpanel status
pgpanel logs
# Böngésző: https://YOUR_DOMAIN/setup
# user: admin (vagy amit megadtál)
# jelszó: a telepítő által kiírt / .admin-password-ONCE
```

---

## 4. Gyors troubleshooting

| Probléma | Megoldás |
|----------|----------|
| `pgpanel` a URL-ben | Állítsd `PGPANEL_REPO`-t vagy szerkeszd `install-pgpanel.sh`-t |
| Nincs jelszó | Nézd: `/etc/pgpanel/.admin-password-ONCE` és a install log végét |
| Setup 403 bootstrap | Másold a `PGPANEL_BOOTSTRAP_TOKEN`-t az `.env`-ből a setup formba |
| curl\|bash nem interaktív | A menü TTY-t vár; futtasd SSH-n interaktívan, ne pure CI pipe-ból kérdés nélkül |
| Privát repo | Clone tokennel/SSH-vel, ne raw.githubusercontent.com |

---

## 5. Biztonsági megjegyzés a `curl | sudo bash` mintához

Ez kényelmes, de a script tartalmát a futtatás előtt érdemes megnézni:

```bash
curl -sSL https://raw.githubusercontent.com/pgpanel/pgpanel/main/install-pgpanel.sh | less
# majd:
curl -sSL ... | sudo bash
```
