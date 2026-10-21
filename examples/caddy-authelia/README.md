# Caddy + Authelia example

SSO in front of myrss via forward-auth; the app trusts `Remote-User` only from
the Caddy container (`trusted_peer_cidrs`). Reader apps keep working: their
compat APIs bypass SSO and use per-app credentials created in the UI.

1. Replace `example.com`, `your-authelia-username`, and every `change-me`.
2. Put a real password hash in `authelia/users_database.yml` (see file).
3. Pin `trusted_peer_cidrs` to your Caddy container's address (a compose
   `networks` block with a static IP is the easy way — Caddy must come from a
   fixed IP for the CIDR check to hold).
4. `docker compose up -d`.
