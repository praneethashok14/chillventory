# Progressive Web App — StockSpace

Make StockSpace an installable PWA while keeping the application frontend in one index.html.

## Required support files
- manifest.webmanifest with name, short name, start URL, standalone display mode, theme colour, background colour, and icon declarations.
- Appropriate app icons in the sizes required by target browsers.
- A separate service-worker.js where required by browser rules.
- Rust server routes that serve these assets with correct content types.

## Installation
Support browser-provided installation on compatible desktop and mobile browsers. Do not promise every browser shows an install button. Keep the website usable without installation. Document installation steps.

## Offline behaviour
Cache only safe static app-shell resources. Never cache PINs, authentication secrets, authenticated inventory API responses, or private inventory data. The backend remains the source of truth.

If offline inventory access is not implemented, explain that inventory requires a connection to the backend. Never claim an offline mutation was saved. Show connection and save status clearly.

## Security
- PWA installation must not bypass PIN authentication.
- A cached app shell must still require authentication before showing private data.
- Use HTTPS in production; localhost development may use browser-trusted localhost HTTP.
- Do not put secrets in URLs, the manifest, or service-worker code.
- Ensure logout/lock removes private in-memory UI state and does not leave private content accessible from a cached page.

## Compatibility tests
Verify manifest loading, icon loading, service-worker registration, installation, refresh, restart, and authenticated API access. Verify that private API responses are not cached.