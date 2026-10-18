# Rust Backend and JSON Persistence — StockSpace

## Stack
Use Rust, Axum, Tokio, Serde, and serde_json. Keep the implementation maintainable and dependencies minimal.

## Persistence
Store appliances, groups, variants, and inventory records in documented JSON files. Store authentication configuration separately from inventory data.

- Initialize an empty file only when it does not exist.
- If an existing file is malformed or unreadable, report the issue and preserve the original.
- Write to a temporary file and replace the destination atomically where supported.
- Serialize mutations to avoid lost updates.
- Keep in-memory and on-disk state consistent if a save fails.
- Persist group relationships, pack states, and quantities across restarts.
- Never expose data or authentication files as static web assets.
- Restrict file access to configured application storage; never accept arbitrary filesystem paths from clients.

## Validation
Validate IDs, names, dates, quantities, units, appliance assignments, group references, and pack states. Reject negative quantities and invalid numeric values. Verify that referenced records exist.

## Server
Serve index.html, PWA support assets, and the API from the same origin. Bind to localhost by default. Use appropriate status codes and structured JSON errors.

## Security integration
Implement the authentication rules in 11-PIN-ACCESS-AND-SECURITY.md. Every private read and mutation must be authenticated. Use secure session handling and protect state-changing requests.

## Data integrity
Pack transfers and quantity adjustments must be atomic. A failed mutation must not be reported as successful. Avoid silent data loss when upgrading the data model.

## Tests
Use isolated temporary data files for tests and never modify real user inventory during tests.