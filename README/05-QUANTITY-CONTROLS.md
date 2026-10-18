# Quantity Controls — StockSpace

Every visible inventory record must have visible − and + buttons directly beside its displayed quantity or pack count. They must not depend on hovering.

## Behaviour
- + increases the selected record's quantity.
- − decreases that record's quantity.
- Pack counts change in whole-pack increments.
- Individual quantities support sensible unit-aware increments and fractional values where appropriate.
- Never allow negative results, NaN, or infinity.
- Send every change to the Rust backend.
- Persist successful updates to JSON.
- Return and display the backend-confirmed value.
- If an update fails, retain the last confirmed value and show an error.
- Handle rapid clicks and concurrent requests without lost updates.
- Disable or otherwise guard controls during pending operations where necessary.

## Accessibility
Use real buttons with accessible labels such as Increase quantity and Decrease quantity. Keep touch targets usable on mobile.

Each child variant has independent controls. Never apply a parent's controls to its children or double-count group quantities.

## API
Provide an adjustment endpoint such as:
POST /api/appliances/:id/items/:item_id/adjust
Body: {"delta": 1}

Validate the delta and resulting quantity server-side. Apply changes atomically and persist them before reporting success.

## Tests
Test increases, decreases, fractional values, invalid adjustments, concurrent updates, and save failures.