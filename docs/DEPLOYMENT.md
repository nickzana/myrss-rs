# Deployment

## Docker Compose

```sh
cp config.example.toml config.toml
mkdir -p secrets
printf 'choose-a-long-owner-password' > secrets/owner-password.txt
chmod 600 secrets/owner-password.txt
docker compose up -d --build
```

Edit only `config.toml`. Keep the default local-auth password in `secrets/owner-password.txt`, mounted as `/run/myrss-secrets/myrss_owner_password`.

## Proxy

Terminate TLS at the proxy and forward to `http://myrss:8080`. Preserve `Host`/`Forwarded` only if the proxy is configured correctly; `public_url` remains the canonical browser URL. Native API paths (`/api/greader.php`, `/api/fever.php`) must bypass interactive SSO redirects but still reach the application's credential checks.

## OIDC or forward auth

Enable one mode in `config.toml`: local, OIDC, or trusted proxy. Put OIDC client secrets in Docker Secrets and use `client_secret_file`. For trusted proxy mode, set exact peer CIDRs and the expected user value.

## Backup and restore

```sh
docker compose exec --user myrss myrss myrss --config /etc/myrss/config.toml backup /var/lib/myrss/backup.db
```

Restore by stopping the service, replacing `myrss.db` and its `-wal`/`-shm` files with the backup, then starting it. Do not copy only a live `.db` file.

## Migrate from FreshRSS SQLite

Stop FreshRSS, then run the importer against its user database (`data/users/<name>/db.sqlite`):

```sh
docker compose stop myrss
docker compose run --rm \
  -v /path/to/db.sqlite:/freshrss-data:ro \
  myrss --config /etc/myrss/config.toml import-freshrss /freshrss-data
docker compose up -d myrss
```

It accepts a FreshRSS `data` directory, a `data/users/<name>` directory, or a `db.sqlite` path. It imports categories as folders, feeds and their HTTP credentials, articles, read/starred state, and labels. FreshRSS MySQL/PostgreSQL databases should be migrated/exported to SQLite or OPML first; OPML imports subscriptions only.
