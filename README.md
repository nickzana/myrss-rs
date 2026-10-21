# myrss

A single-user, self-hosted RSS reader in Rust. It combines feed fetching, a fast embedded web UI, SQLite storage, Google Reader/FreshRSS-compatible endpoints, and an optional Fever endpoint in one binary.

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

Docs: [architecture](docs/ARCHITECTURE.md), [security](docs/SECURITY.md), [deployment](docs/DEPLOYMENT.md), [user guide](docs/USER_GUIDE.md).

MIT licensed.
