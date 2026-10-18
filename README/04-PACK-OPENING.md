# Pack Opening Actions — Fridgeflow

Every unopened pack variant with a pack count greater than zero must offer:
1. Mark ×1 Open
2. Mark All Open

## Mark ×1 Open
Transfer exactly one pack from the unopened variant to its matching opened variant. Decrease unopened count by one and increase opened count by one.

## Mark All Open
Transfer every remaining unopened pack to the matching opened variant. Set unopened count to zero and increase opened count by the transferred amount.

## Matching variant
Find the opened variant in the same parent group and appliance. If it does not exist, create it automatically. Preserve the pack label, appliance, group, and relevant metadata. If metadata conflicts, use a documented rule and do not silently overwrite user data.

## Atomicity
Perform the entire transfer as one backend transaction/locked mutation. Do not implement it as separate client-side updates. Save successfully before reporting success.

- Total packs before and after must be identical.
- Never duplicate, lose, or create packs during a transfer.
- Failed operations must leave the inventory unchanged.
- Prevent duplicate transfers from concurrent or repeated requests.
- Disable buttons while a request is in progress.
- Show a clear success or error message.
- Refresh all relevant views after success.

## Example
Start: 4 unopened, 0 opened.
Mark ×1 Open: 3 unopened, 1 opened.
Mark All Open: 0 unopened, 4 opened.

Only show these actions on eligible unopened variants. Quantity adjustments alone must never change pack status.

## Tests
Test both actions, automatic variant creation, concurrent calls, persistence failures, and pack-count conservation.