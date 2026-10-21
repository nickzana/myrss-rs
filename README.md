# myrss

A single-user, self-hosted RSS reader in Rust. One binary: feed fetching, a
fast embedded web UI, SQLite storage, Google Reader/FreshRSS-compatible
endpoints, and an optional Fever endpoint.

![Reader](docs/screenshots/reader.png)

- Three auth modes: local password, OIDC (auth code + PKCE), trusted proxy
  header — see [architecture](docs/ARCHITECTURE.md).
- Reader apps connect with named, revocable credentials over
  `/api/greader.php` or `/api/fever.php` — verified with NetNewsWire and
  Read You.
- FreshRSS SQLite import covers feeds, folders, articles, read/starred state,
  and labels.

## Run with Docker

```sh
cp config.example.toml config.toml
mkdir -p secrets
printf 'choose-a-long-owner-password' > secrets/owner-password.txt
chmod 600 secrets/owner-password.txt
docker compose up -d --build
```

Open `http://localhost:8080`, sign in, then add a feed or import OPML.

Useful commands:

```sh
myrss --config config.toml check-config
myrss --config config.toml backup backups/myrss.db
myrss --config config.toml create-credential netnewswire
myrss --config config.toml import-freshrss /path/to/freshrss/data
```

Docs: [architecture](docs/ARCHITECTURE.md), [security](docs/SECURITY.md),
[deployment](docs/DEPLOYMENT.md), [user guide](docs/USER_GUIDE.md).

Examples: [Caddy + Authelia forward-auth](examples/caddy-authelia).

MIT licensed.
