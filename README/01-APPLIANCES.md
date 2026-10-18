# Appliances — Chillventory

Implement management of multiple storage appliances.

## Fields
- Unique ID
- Name, e.g. Kitchen Fridge or Garage Freezer
- Type: fridge, freezer, or other
- Optional location and description
- Creation and update timestamps

## Operations
- Create, list, retrieve, rename, edit, and delete appliances.
- View all inventory belonging to an appliance.
- Assign inventory items and groups to appliances.
- Show each item's appliance clearly in global inventory.
- Persist every change through the backend.

## Deletion safety
Before deleting an appliance, clearly explain what will happen to its inventory. Offer moving contents to another appliance or deleting them only after explicit confirmation. Never silently destroy inventory.

## Validation
- IDs must be unique and valid.
- Reject invalid references.
- Handle missing appliances with clear errors.
- Update related UI views immediately after successful changes.

## Tests
Test CRUD operations, invalid IDs, persistence, and safe handling of associated inventory.