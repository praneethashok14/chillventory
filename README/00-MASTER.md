# Chillventory — Master Specification

## Mission
Build Chillventory: an installable, PIN-protected website for manually tracking food and other items in multiple fridges and freezers.

Read every specification from 01 through 11 before implementing anything. Treat them as one coherent application, not separate projects.

## Architecture
- Frontend application: exactly one index.html containing all HTML, CSS, and application JavaScript, using vanilla JavaScript only.
- Backend: Rust, Axum, Tokio, Serde, and serde_json.
- Persistence: JSON data files on the backend.
- PWA support files may include a web manifest, icons, and a separate service worker.
- Serve the frontend, PWA assets, and API from the same origin.
- No frontend framework, frontend build system, smart-fridge integration, barcode scanning, receipt scanning, or automatic item recognition.
- Bind to localhost by default for local use. Document safe deployment for access from other devices.

## Specification files
1. 01-APPLIANCES.md — fridges and freezers
2. 02-INVENTORY-ITEMS.md — inventory records
3. 03-GROUPS-AND-VARIANTS.md — parent groups and child variants
4. 04-PACK-OPENING.md — Mark ×1 Open and Mark All Open
5. 05-QUANTITY-CONTROLS.md — plus/minus quantity controls
6. 06-DASHBOARD-AND-SEARCH.md — dashboard, search, expiration
7. 07-UI-AND-ACCESSIBILITY.md — interface and accessibility
8. 08-BACKEND-AND-PERSISTENCE.md — Rust backend and JSON storage
9. 09-API-AND-TESTING.md — REST API and tests
10. 10-PWA-INSTALLATION.md — installable PWA
11. 11-PIN-ACCESS-AND-SECURITY.md — PIN authentication and security

## Core data model
An appliance contains inventory groups and/or ungrouped items. A group is an organizational parent with child variants. A group is not itself a quantity-bearing inventory record.

Individual-quantity items track an amount and unit. Pack-based variants track a pack count, pack label, and explicit Opened or Unopened status. Opened and unopened stock must be independently represented.

## Non-negotiable requirements
- Every inventory mutation is validated by the backend and persisted successfully before being reported as successful.
- Every visible inventory record has visible − and + controls beside its quantity.
- Unopened pack variants have Mark ×1 Open and Mark All Open actions when their count is greater than zero.
- Group totals never double-count children.
- Authentication protects all private inventory API operations.
- PINs are never stored in plaintext.
- Authenticated inventory responses must never be cached by the service worker.
- PWA installation must not bypass authentication.
- Do not silently overwrite corrupt inventory data.
- Handle concurrent changes safely.
- Never inject user-provided text as HTML.
- Provide clear errors, loading states, empty states, and destructive-action confirmations.

## Suggested project structure
chillventory/
  Cargo.toml
  Cargo.lock
  README.md
  index.html
  manifest.webmanifest
  service-worker.js
  src/
    main.rs
    models.rs
    routes.rs
    storage.rs
    auth.rs
  data/
    inventory.json
    auth.json
  icons/

Adapt the structure where justified, but retain a single frontend application file. Protect data files from public HTTP access.

## Implementation workflow
1. Read all specifications.
2. Design one consistent data model and API.
3. Implement authentication and persistence.
4. Implement appliances, items, groups, and atomic pack transfers.
5. Build the complete frontend.
6. Add PWA support without caching private data.
7. Write automated tests.
8. Run cargo fmt --check, cargo check, and cargo test.
9. Provide a README with setup, run, security, backup, installation, and testing instructions.

Never claim a test or command succeeded unless it was actually executed.