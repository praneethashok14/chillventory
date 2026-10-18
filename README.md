# Fridgeflow

A PIN-protected, installable PWA for manually tracking food (and anything else) across multiple fridges and freezers. The frontend is a single `index.html` (vanilla JavaScript, no build tools). Two interchangeable backends ship in the repo: a Rust/Axum/Tokio server with JSON-file persistence (great for local development) and a TypeScript/Hono Cloudflare Worker with D1 persistence (the live production deployment).

Each user has their own **account** (name + PIN). Accounts are fully isolated — two people on the same server can even pick the same PIN and never see each other's data.

Spec source: the `README/` directory in this repository (files `00-MASTER.md` through `11-PIN-ACCESS-AND-SECURITY.md`).

## 🚀 Live deployment

- **Production URL:** <https://fridgeflow.praneethashok14.workers.dev>
- **Hosted on:** Cloudflare Workers (free tier) with D1 (SQLite) for durable storage, KV for sessions.
- **Deployed from:** `cloudflare/` subdirectory in this repo. See [`cloudflare/README.md`](cloudflare/README.md) for the full Workers deployment guide, including how to redeploy, back up D1, and roll back a bad version.
- **First visit** prompts you to create an account; subsequent visits unlock with your name + PIN.
- **Install as a PWA** (home-screen icon) from the browser's install button or **Share → Add to Home Screen** on iOS.

## Repo layout at a glance

```
.
├── README/                   # Specification docs (00..11).
├── src/                      # Rust/Axum backend (local-dev friendly).
├── tests/                    # Rust integration tests.
├── index.html                # The entire frontend (vanilla JS PWA).
├── manifest.webmanifest      # PWA manifest.
├── service-worker.js         # Service worker (shell only, no API caching).
├── icons/                    # SVG app icons.
├── cloudflare/               # Workers backend + deploy scripts.
│   ├── src/index.ts          # Hono router with the full REST API.
│   ├── wrangler.toml         # D1 + KV + assets bindings.
│   ├── schema.sql            # D1 schema.
│   ├── scripts/deploy.sh     # Deploy helper (double-pins the new version).
│   └── README.md             # Workers deployment guide.
└── Cargo.toml                # Rust package manifest.
```

## Running the Rust backend locally

Prereqs:
- **Rust** 1.75 or newer (stable). If `cargo` is not on your `PATH`, install [rustup](https://rustup.rs/) and reopen your shell.
- No Node.js, bundler, or frontend toolchain required.

From the project root:

```sh
cargo run --release
```

You should see:

```
fridgeflow listening on http://127.0.0.1:8787
```

Open <http://127.0.0.1:8787> in your browser. On your first visit the UI prompts you to create an account (name + 4–12 digit PIN). The signup screen has a **"Login with existing account"** button, and the login screen has **"Or create a new account"** to switch back.

### Environment variables

| Variable                 | Default            | Purpose                                                                     |
| ------------------------ | ------------------ | --------------------------------------------------------------------------- |
| `FRIDGEFLOW_BIND`      | `127.0.0.1:8787`   | Address the HTTP server binds to.                                           |
| `FRIDGEFLOW_DATA_DIR`  | `./data`           | Root of all persisted data.                                                 |
| `FRIDGEFLOW_ASSET_DIR` | `.`                | Where `index.html`, `manifest.webmanifest`, `service-worker.js`, and `icons/` live. |

By default the server binds **localhost**. See "Remote access" below before changing that.

## Data on disk

```
data/
├── accounts.json                        # list of accounts (id, name, argon2 pin hash, timestamps)
└── accounts/
    ├── <account-id>/                    # one folder per account, named by random UUID
    │   └── inventory.json
    └── <account-id>/
        └── inventory.json
```

Folders are named by random account UUIDs — **not** by the PIN or by any value derivable from the PIN. PINs appear nowhere on disk; `accounts.json` only ever contains salted Argon2 hashes.

All writes are atomic (temp file + `rename`). If the server finds a malformed file on startup it refuses to overwrite it; restore from backup.

### Backup

While the server is **stopped**, copy `data/accounts.json` and the `data/accounts/` directory somewhere safe. Restoring is just the reverse.

## Accounts, PINs, and recovery

- Account names are 1–60 characters, case-insensitively unique.
- PINs are 4–12 ASCII digits. Two accounts can share the same PIN; names must differ.
- PINs are stored only as Argon2 hashes with per-account random salts.
- A successful login creates a server-side session identified by a random, HttpOnly, SameSite=Strict cookie (`fridgeflow_session`).
- Mutations also require a CSRF header (`x-fridgeflow-csrf`) matching the token issued at login/signup. The frontend sends it automatically.
- Sessions expire after 1 hour of inactivity. Manual lock + change-pin both revoke every active session for the account.
- Repeated failed login attempts from the same IP are rate-limited with progressive backoff (5s → 30s → 2m → 5m).
- Wrong **name** and wrong **PIN** both return the same generic `incorrect name or PIN` — the server runs a dummy argon2 verification on unknown names so the timing matches, preventing name enumeration.

### Forgotten PIN

There is **no master PIN and no backdoor**.

- **Forgot one account's PIN?** Stop the server, open `data/accounts.json`, remove just that account's entry, then start the server again. Also delete `data/accounts/<that-account-id>/` if you want to drop the inventory, or keep it and re-sign-up with any name to generate a new account (its data folder gets a new UUID; the orphan folder stays until you clean it up).
- **Lost all PINs?** Delete `data/accounts.json`. Every account resets to "no account yet" on next launch. The `data/accounts/*/inventory.json` folders are preserved but no account points at them until you manually re-associate (edit JSON) or delete them.

Changing a PIN does not touch inventory data.

## API

Every endpoint under `/api/*` except those listed below requires an authenticated session cookie.

### Public

| Method | Path                | Purpose                                                                   |
| ------ | ------------------- | ------------------------------------------------------------------------- |
| GET    | `/api/health`       | Liveness check. `{status:"ok"}`.                                          |
| GET    | `/api/auth/status`  | `{has_accounts, account_count, account_names}` — no inventory data.       |
| GET    | `/api/auth/session` | `{authenticated, csrf_token?, account_id?, account_name?}`.               |
| POST   | `/api/auth/signup`  | Body `{name, pin}`. Creates account; sets session cookie.                 |
| POST   | `/api/auth/login`   | Body `{name, pin}`. Verifies and sets session cookie.                     |
| POST   | `/api/auth/lock`    | Revokes the current session and clears the cookie.                        |

### Authenticated

| Method           | Path                                                     | Purpose                                                   |
| ---------------- | -------------------------------------------------------- | --------------------------------------------------------- |
| POST             | `/api/auth/change-pin`                                   | Body `{current_pin, new_pin}`. Revokes all sessions.      |
| GET, POST        | `/api/appliances`                                        | List / create appliances in the current account.          |
| GET, PATCH, DELETE | `/api/appliances/:id`                                  | Retrieve / update / delete an appliance.                  |
| GET, POST        | `/api/appliances/:id/items`                              | List / create items for an appliance.                     |
| PATCH, DELETE    | `/api/appliances/:id/items/:item_id`                     | Edit metadata / delete an item.                           |
| POST             | `/api/appliances/:id/items/:item_id/adjust`              | Body `{delta}`. Atomic quantity change.                   |
| POST             | `/api/appliances/:id/items/:item_id/open-one`            | Transfer 1 unopened pack → matching opened variant.       |
| POST             | `/api/appliances/:id/items/:item_id/open-all`            | Transfer all unopened packs → matching opened variant.    |
| GET, POST        | `/api/groups`                                            | List / create groups.                                     |
| PATCH, DELETE    | `/api/groups/:id`                                        | Rename, re-home, or delete a group.                       |
| GET              | `/api/items`                                             | Flat list of all appliances, groups, and items.           |

#### Deleting things with contents

Deleting an appliance that has any items or groups returns **409** unless you pass a strategy:

- `?strategy=cascade` — delete every item and group it contains.
- `?strategy=move&move_to=<other_appliance_id>` — reassign contents.

Deleting a group with children similarly requires `?strategy=detach|move|cascade` (`move` also takes `move_to_group=<other_group_id>`).

### Pack-opening semantics

An **unopened** pack-based item is the only kind that shows `Mark ×1 Open` and `Mark All Open`. The transfer:

- Finds (or auto-creates) the matching **opened** variant in the same appliance, group, and pack label.
- Decrements the unopened count and increments the opened count under one lock + atomic file write.
- Preserves total pack count. Covered by integration tests under concurrent calls.

### Delete-on-zero (frontend convenience)

When the user presses **−** and the resulting quantity would be zero (or below for fractional units that over-shoot), the frontend calls `DELETE` on the item instead of `adjust`. The server-side `adjust` endpoint still rejects negative resulting quantities on its own — the auto-delete is purely a UI nicety.

## PWA installation

The site is installable in Chromium-based browsers and iOS Safari.

- Chrome / Edge: look for the install icon in the address bar, or **Menu → Install app**.
- Safari (iOS): tap **Share → Add to Home Screen**.
- Firefox desktop does not currently show an install button; the site remains fully usable in the browser.

Installing the app does **not** bypass authentication: the backend still demands PIN login on every unlock, and the cached shell never contains private data.

### Offline behaviour

The service worker caches **only** the public app shell (`/`, `index.html`, `manifest.webmanifest`, the icons, and the worker itself). It refuses to cache anything under `/api/*`. Consequence: you can open the app offline and see the lock screen, but inventory data requires the backend to be reachable. There is no offline writes queue — a save is only successful once the backend confirms it.

## Remote access

The server binds to `127.0.0.1` by default so nothing is reachable from other machines without explicit action. If you want to use Fridgeflow from your phone on the same Wi-Fi, the recommended pattern is:

1. Set `FRIDGEFLOW_BIND=0.0.0.0:8787`.
2. Put the server behind a reverse proxy (Caddy, nginx) that terminates **HTTPS** on your LAN.
3. Restrict access with your router's firewall or a VPN.

**Do not expose the server directly to the public internet without HTTPS.** Numeric PINs have limited entropy on their own; HTTPS plus server-side rate limiting is what makes the setup safe.

## Running the tests

From the project root:

```sh
cargo fmt --check
cargo check
cargo test
```

Tests spin up isolated servers against temporary data directories, so they never touch your real `data/` folder. The suite covers: appliance/item/group CRUD, +/− adjustments (incl. 20 concurrent requests → exactly +20), pack opening with auto-created opened variants and conservation, deletion-safety strategies, session expiry, lock/change-PIN session revocation, CSRF enforcement, two-account isolation (same PIN, different data), corrupt-file preservation, and full persistence across restart.

## Rust backend — detailed layout

```
.
├── Cargo.toml
├── README/                       # The source specifications (00..11).
├── src/
│   ├── main.rs                   # Server bootstrap (Tokio + Axum).
│   ├── lib.rs                    # Re-exports modules for tests.
│   ├── models.rs                 # Appliance, Group, Item, tracking modes.
│   ├── storage.rs                # Atomic JSON persistence + locking.
│   ├── accounts.rs               # Multi-account PIN auth + sessions + rate limiting.
│   ├── routes.rs                 # REST handlers (account-scoped).
│   └── static_files.rs           # Serves index.html, manifest, SW, icons.
├── tests/
│   ├── api.rs                    # End-to-end integration tests.
│   └── common/mod.rs             # Test harness.
├── index.html                    # The entire frontend.
├── manifest.webmanifest          # PWA manifest.
├── service-worker.js             # App-shell service worker.
├── icons/                        # App icons (SVG).
└── data/                         # Created at runtime; holds accounts.json + accounts/<id>/inventory.json.
```

## Workers backend (deployed)

The TypeScript/Hono port under [`cloudflare/`](cloudflare/) is what serves the live site. API shape is identical to the Rust server. Key differences:

- **Storage:** D1 (SQLite) instead of filesystem — `accounts`, `inventories`, and `item_photos` tables. Fully safe across `wrangler deploy` (data lives in your Cloudflare account, not in the Worker bundle).
- **PIN hashing:** PBKDF2-SHA256 at 100k iterations via Workers' Web Crypto (Workers caps PBKDF2 at 100k; iteration count is stored in each hash, so bumping the config var is safe).
- **Photos:** JPEG BLOBs in D1 (no R2 required).
- **Deploys:** `cd cloudflare && npm run deploy` — the helper script in `cloudflare/scripts/deploy.sh` captures the version ID from `wrangler deploy` and pins it to 100% traffic twice (with a wait in between) to defeat a wrangler v4 quirk where a deferred second deploy can override the first.
- **Backups:** `npm run db:backup` dumps the whole D1 to a timestamped SQL file under `cloudflare/backups/` (gitignored).
- **D1 Time Travel:** Automatic 30-day point-in-time restore included — see the Workers README for the `wrangler d1 time-travel` commands.

Deployment and recovery details are all in [`cloudflare/README.md`](cloudflare/README.md).

## License

See `Cargo.toml` for the declared license.
