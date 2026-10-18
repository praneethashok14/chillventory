# API and Testing — Fridgeflow

## REST API
Implement consistent JSON responses and backend validation.

Required endpoints:
- GET /api/health
- GET, POST /api/appliances
- GET, PATCH, DELETE /api/appliances/:id
- GET, POST /api/appliances/:id/items
- PATCH, DELETE /api/appliances/:id/items/:item_id
- POST /api/appliances/:id/items/:item_id/adjust
- GET /api/items

Add appropriate group, variant, pack-opening, PIN setup/unlock, session, lock, and PIN-change endpoints. Document all routes and which require authentication.

## Quantity adjustment
Example body: {"delta": 1}. Negative deltas decrease quantities. Validate the delta and resulting value. Apply updates atomically, persist successfully, and return the updated record. Reject invalid requests and negative resulting quantities.

## Pack-opening operations
Mark ×1 Open transfers exactly one pack. Mark All Open transfers all remaining unopened packs. Create the opened variant if needed. Both operations must preserve pack totals and run as atomic backend mutations.

## Authentication
All private inventory reads and mutations must reject unauthenticated requests. Authentication and session endpoints must not reveal private inventory. Test session expiry and revocation.

## Error responses
Use appropriate status codes, such as 200, 201, 204, 400, 401, 403 where applicable, 404, 409, 413 if size limits are exceeded, 429 for rate limiting, and 500 for unexpected failures. Use structured errors with readable messages. Never expose secrets or stack traces.

## Automated tests
Test:
- Appliance CRUD and deletion safety.
- Item CRUD and validation.
- Group/variant relationships and non-duplicated totals.
- Quantity increase/decrease and invalid values.
- Concurrent quantity changes.
- Mark ×1 Open and Mark All Open.
- Automatic opened-variant creation.
- Pack-count conservation and concurrent transfers.
- JSON persistence across restart.
- Corrupt-file preservation and write failures.
- Authentication-required endpoints.
- Incorrect PIN, session expiry, logout, and rate limiting.
- PWA shell availability without private-data caching.
- Missing resources and malformed JSON.

Use temporary isolated data for tests.

## Acceptance commands
Run from the project directory:
cargo fmt --check
cargo check
cargo test

Fix failures and rerun checks. Report actual results; never claim an unexecuted check passed.

## README
Document prerequisites, one-command startup, server address, data locations, backups, API routes, authentication and recovery, PWA installation, and tests.

## Definition of done
The documented API works, mutations persist reliably, authentication protects inventory, pack transfers cannot duplicate or lose packs, and tests accurately reflect the implementation.