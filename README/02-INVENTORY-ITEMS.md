# Inventory Items — StockSpace

Users manually create, edit, and delete inventory records.

## Common fields
- Unique ID
- Name
- Optional parent group ID
- Appliance ID
- Tracking mode: individual quantity or pack-based
- Optional category
- Quantity or pack count
- Unit or pack label
- Optional expiration date
- Optional notes
- Creation and update timestamps

## Individual-quantity mode
Track amounts such as 2 bottles, 500 g, or 1.5 L. Support fractional amounts when appropriate. Store the unit explicitly and never allow negative quantities.

## Pack-based mode
Track a whole-number pack count and a pack label, such as bags, boxes, packets, or tubs. Every record must explicitly have Opened or Unopened status. Use separate records for opened and unopened packs.

## Operations
- Create, retrieve, edit, and delete records.
- Validate required names, units, dates, quantities, and references.
- Preserve optional fields during updates unless the user intentionally changes them.
- Confirm destructive deletion where appropriate.
- Persist all changes through the Rust backend.

## Rules
Do not infer that a pack has opened just because its quantity changes. Do not merge different pack states automatically. Do not add barcode scanning, receipt scanning, smart-fridge integration, or automatic recognition.

## Tests
Test both tracking modes, validation, fractional quantities, deletion, and persistence.