# Groups and Variants — Chillventory

Implement parent groups that organize related inventory records.

## Example
Parent group: Peas
- Opened pack of Peas — pack-based, Opened
- Unopened pack of Peas — pack-based, Unopened

## Data model
A group has a unique ID, name, appliance assignment, optional description, and child records. A group is an organizational container, not a stock record. Never count the parent as extra inventory.

Each child record maintains its own quantity, unit or pack label, pack status, expiration date, and notes.

## Operations
- Create, rename, and delete groups.
- Add, rename, edit, and delete child variants.
- Move variants between groups or make them ungrouped.
- Allow users to choose names manually.
- Search and filter by both group and variant name.
- Persist group relationships in JSON.

## Display
Show group names prominently and child variants nested beneath them. Support expanding and collapsing groups. Each child keeps its own quantity controls and eligible pack-opening actions.

Groups and variants must display consistently in the dashboard, appliance pages, global inventory, search, and expiration views.

## Safety
Deleting a variant must not delete siblings. Deleting a group must explain what happens to its children and require confirmation. Never silently discard inventory.

## Counting
Calculate totals from child records only. Do not merge opened and unopened counts or statuses automatically.

## Tests
Test group CRUD, child relationships, moves, deletion safety, persistence, and correct totals.