# UI and Accessibility — Chillventory

Build a clean, responsive interface for desktop, tablet, and mobile.

## Frontend architecture
All application HTML, CSS, and JavaScript must be in one index.html using vanilla JavaScript. PWA support files such as the manifest, icons, and service worker may be separate.

## Navigation
Provide Dashboard, Appliances, and All Inventory views. Provide PIN setup/unlock, manual locking, and session-expiry handling. Do not show private inventory before authentication.

## Inventory UI
Show names, appliances, quantities, units, pack states, and expiration dates clearly. Nest child variants under parent groups. Keep − and + controls visible beside every record's quantity. Show Mark ×1 Open and Mark All Open for eligible unopened variants.

## Feedback
Provide loading indicators, empty states, validation messages, success and error feedback. Confirm destructive actions. Do not claim a save succeeded before the backend confirms it.

## Themes
Provide light and dark themes. Persist the appearance preference locally if desired, but never store authentication secrets or private inventory in localStorage.

## Accessibility
- Use semantic HTML and associated form labels.
- Support keyboard navigation and visible focus indicators.
- Give icon-only controls accessible names.
- Do not rely on colour alone to convey state.
- Respect reduced-motion preferences.
- Maintain readable contrast and mobile-friendly touch targets.

## Security
Render user-provided text safely as text, never trusted HTML. Avoid eval and executable code derived from user input. Enforce authentication and validation in the backend, not just the interface.