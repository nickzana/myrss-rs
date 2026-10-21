# Architecture

myrss is one Rust process: an Axum HTTP server, embedded static UI, scheduler, guarded feed fetcher, parser/sanitizer, and SQLite database. There are no queues, workers, Node services, or secondary databases.

Browser, native JSON, Google Reader, and Fever APIs share the same feed and entry tables. Feed I/O happens outside database transactions; SQLite uses WAL, bounded connections, and transactional entry/state writes.

Trust boundaries: the database is authoritative; remote feeds are hostile; native app credentials are independent of browser login; OIDC and proxy identity are optional alternatives to local login.
