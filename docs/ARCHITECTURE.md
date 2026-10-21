# Architecture

myrss is one Rust process: an Axum HTTP server, embedded static UI, scheduler,
guarded feed fetcher, parser/sanitizer, and SQLite database. There are no
queues, workers, Node services, or secondary databases.

```mermaid
flowchart LR
    subgraph clients[Clients]
        B[Browser UI]
        N[NetNewsWire / Read You]
    end
    subgraph myrss[myrss process]
        UI[Embedded static UI]
        API[/api/v1 native/]
        GR[/api/greader.php/]
        FV[/api/fever.php/]
        SCH[Refresh scheduler]
        FCH[Guarded fetcher]
        SAN[feed-rs + ammonia]
        DB[(SQLite WAL)]
    end
    FEEDS[Remote feeds]

    B --> API --> DB
    B --> UI
    N --> GR --> DB
    N --> FV --> DB
    SCH --> FCH --> FEEDS
    FCH --> SAN --> DB
```

- Browser, native JSON, Google Reader, and Fever APIs share the same feed and
  entry tables and the same auth module.
- Feed I/O happens outside database transactions; SQLite uses WAL, bounded
  connections, and transactional entry/state writes.
- Fetching is bounded (size, time, redirects) and rejects private-network
  targets unless explicitly allowed — see [security](SECURITY.md).

## Auth modes

Exactly one of three modes is active, selected by `auth.mode`:

```mermaid
flowchart TB
    R[Request] --> M{auth.mode}
    M -->|local| L[owner password<br>+ session cookie]
    M -->|oidc| O[OIDC auth code + PKCE<br>+ session cookie]
    M -->|proxy| P[trusted peer CIDR<br>+ identity header]
    L --> S[Session + CSRF token]
    O --> S
    P --> API2[Per-request identity]
```

App credentials for reader clients are independent of all three modes and
authenticate only the compat APIs (`/api/greader.php`, `/api/fever.php`) and the
native credential-scoped calls.
