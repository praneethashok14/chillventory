# Fridgeflow on Cloudflare Workers

An optional TypeScript port of the backend that runs the whole app on Cloudflare's edge, using **D1** (SQLite) for durable storage and **KV** for session tokens. The original Rust server at the repo root still works for local development; this directory is independent.

Data persists across every `wrangler deploy`. Deploys replace the Worker **code only** — D1 rows and KV entries live in your Cloudflare account and keep every value.

## One-time setup

Prereqs: Node 20+, a Cloudflare account.

```sh
cd cloudflare
npm install
npx wrangler login                                    # browser OAuth, one time
npx wrangler d1 create fridgeflow                   # prints a database_id
npx wrangler kv:namespace create SESSIONS             # prints an id
```

Open `wrangler.toml` and paste the two IDs into the `REPLACE_WITH_...` placeholders.

Then initialize the schema:

```sh
npm run db:init           # runs schema.sql against the remote D1
```

(For local-only testing with `wrangler dev`, use `npm run db:init:local` instead or additionally.)

## Deploy

```sh
npm run deploy
```

The `prepare-assets` step copies `index.html`, `manifest.webmanifest`, `service-worker.js`, and `icons/` from the repo root into `cloudflare/public/`, then `wrangler deploy` ships both the Worker and the static shell under one domain. The URL Wrangler prints at the end is where your app lives (something like `https://fridgeflow.<your-subdomain>.workers.dev`).

Open it in a browser, create your account, done.

## Dev loop

```sh
npm run dev
```

Serves the Worker locally on `http://127.0.0.1:8787` with the real D1 and KV bindings (unless `wrangler.toml` points them at `--local` databases).

## What this gives up vs. the Rust server

- **PIN hashing** uses PBKDF2-SHA256 via the Workers Web Crypto API, not Argon2. PBKDF2 is slightly weaker per-CPU-cycle than Argon2 but is native to the Workers runtime. The current iteration count is **100,000** (Workers' SubtleCrypto hard-caps PBKDF2 at this value — attempting more raises `NotSupportedError`). The iteration count is stored inside each hash string, so you can tune `PBKDF2_ITERATIONS` without invalidating existing accounts: verify always uses the iteration count that was baked in when the hash was created.
- **Rate limiting** is not implemented server-side. Add a Cloudflare Rate Limiting rule on `/api/auth/login` and `/api/auth/signup` from the dashboard if you want it. For a tiny private app Cloudflare's default protections usually suffice.
- **Change-PIN session revocation** only revokes the current session's token (KV has no secondary index). Other active sessions on other devices drop out at the 1-hour TTL. Acceptable trade-off for the simpler data model.
- **Photos** are stored as JPEG BLOBs in a dedicated `item_photos` D1 table rather than R2 — fine for small libraries, slower than R2 at scale.

## Data safety on updates

TL;DR: your inventory, accounts, PINs, and photos **persist across every `wrangler deploy`**. The deploy only replaces the Worker code — D1 (rows) and KV (sessions) are separate durable resources owned by your Cloudflare account.

### What survives a code deploy

| State | Lives in | Safe? |
| --- | --- | --- |
| Accounts (name + PIN hash) | D1 `accounts` | ✅ |
| Inventory (appliances/groups/items JSON) | D1 `inventories` | ✅ |
| Photos | D1 `item_photos` | ✅ |
| Sessions | KV `SESSIONS` | ✅ (expire after 1h anyway) |

### What *can* destroy data (don't do these without a backup)

- `wrangler d1 delete fridgeflow`
- `wrangler d1 execute --command "DROP TABLE ..."`
- Destructive migrations in `schema.sql` (anything beyond `CREATE TABLE IF NOT EXISTS` or `CREATE INDEX IF NOT EXISTS` or additive `ALTER TABLE ... ADD COLUMN`)
- Deleting the KV namespace (just logs everyone out — not real inventory loss)

### Three layers of safety net

1. **Cloudflare D1 Time Travel.** Automatic. Point-in-time restore up to **30 days** back, no setup needed, included on the free tier. Docs: <https://developers.cloudflare.com/d1/reference/time-travel/>. To restore to a previous moment:

   ```sh
   npx wrangler d1 time-travel info fridgeflow                # see the current timestamp
   npx wrangler d1 time-travel restore fridgeflow --timestamp=2026-10-18T14:00:00Z
   ```

2. **On-demand SQL dump** of the whole DB to your laptop:

   ```sh
   cd cloudflare
   npm run db:backup
   ```

   Writes `backups/fridgeflow-YYYYMMDD-HHMMSS.sql`. The `backups/` directory is gitignored so your real data never ends up on GitHub. Keep a few of these around if you're about to do something you're unsure about.

3. **Restore from a SQL dump** (recovers a specific snapshot into the live DB):

   ```sh
   npx wrangler d1 execute fridgeflow --remote --file=backups/fridgeflow-20261018-165248.sql
   ```

### Schema change rules

Only add things, never remove, never change types in place. Safe patterns:

- `CREATE TABLE IF NOT EXISTS …`
- `CREATE INDEX IF NOT EXISTS …`
- `ALTER TABLE accounts ADD COLUMN new_field TEXT` (nullable by default — fine)

If you need to remove or rename a column, do it as a multi-step migration (`add new`, `copy data`, `use new in code`, deploy, then separately drop the old) with a `npm run db:backup` right before each phase.

## Files

```
cloudflare/
├── wrangler.toml       # Workers config (D1 binding, KV binding, assets dir)
├── schema.sql          # D1 schema: accounts + inventories + item_photos
├── package.json        # npm scripts: prepare-assets, db:init, db:backup, dev, deploy
├── tsconfig.json       # TS compiler options
├── src/
│   └── index.ts        # Hono app with the full API
├── public/             # (generated by prepare-assets) — gitignored
├── backups/            # local SQL dumps from `npm run db:backup` — gitignored
└── .gitignore
```

## Rolling back a bad deploy

`wrangler deployments list` + `wrangler rollback <id>` reverts the Worker code to a previous version without touching D1 or KV data.
