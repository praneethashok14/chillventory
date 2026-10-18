# PIN Access and Security — Fridgeflow

Protect inventory data with a PIN verified by the Rust backend.

## First setup and unlock
- On first launch, guide the user through setting a PIN.
- On later visits, require the PIN before showing or fetching private inventory.
- Allow manual locking.
- Lock after a configurable inactivity period.
- Provide clear generic feedback for incorrect PINs.
- Do not reveal appliance names, group names, item names, quantities, or expiration dates before authentication.
- Apply identical security to browser and installed PWA use.

## PIN storage
- Never store the PIN in plaintext.
- Never hardcode a default PIN.
- Never store it in localStorage, sessionStorage, URLs, or the service-worker cache.
- Verify the PIN on the backend.
- Store a salted password hash using an established password-hashing algorithm with appropriate parameters.
- Generate unique salts securely.
- Never log or return PINs, hashes, session tokens, or credentials.

## Sessions
- Successful verification creates a server-validated session.
- Protect every private inventory endpoint.
- Use unpredictable session identifiers.
- Prefer an HttpOnly, SameSite cookie, with Secure enabled for HTTPS deployments.
- Protect state-changing requests against CSRF where applicable.
- Never put session tokens in query parameters.
- Locking revokes the session and clears private data from the visible frontend.
- Enforce session expiry and relevant inactivity limits server-side.
- A public health endpoint may exist only if it reveals no private data.

## Brute-force protection
Rate-limit PIN attempts on the server. Add progressive delays or temporary lockouts after repeated failures. Avoid unlimited rapid guessing and avoid messages that reveal sensitive account state.

## Change and recovery
Allow an authenticated user to change the PIN after verifying the current PIN. Document a safe forgotten-PIN recovery process. Do not provide a hidden universal PIN or authentication bypass. Explain recovery consequences clearly. Changing a PIN must not automatically delete inventory.

## Multi-device use
Use one configured backend as the source of truth. If other devices connect to that server, they must use the same origin where practical and authenticate separately. Never expose a localhost-only server to the internet automatically. Document that remote access requires deliberate secure deployment with HTTPS and appropriate network protection.

The PIN is an access gate, not a substitute for HTTPS or secure server deployment. A short PIN has limited entropy, so rate limiting and secure session handling are essential.

## Tests
Verify that:
- Private data is unavailable before authentication.
- Incorrect PINs are rejected.
- Correct PINs establish sessions.
- Unauthenticated API requests are rejected.
- Expired and revoked sessions cannot access inventory.
- Rate limiting works.
- PIN changes require verification.
- Refreshing or reopening the PWA does not bypass authentication.
- Private data is not exposed in caches, URLs, logs, or static assets.