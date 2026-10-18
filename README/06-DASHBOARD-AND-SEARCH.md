# Dashboard, Search, and Expiration — Chillventory

## Dashboard
Show a useful overview of appliances, inventory records, low quantities where meaningful, expired items, and items approaching expiration.

Never double-count parent groups and child variants.

## Global inventory
Show records across all appliances and clearly identify each appliance. Support expanding and collapsing groups. Keep quantity controls and eligible pack-opening actions available.

## Search and filters
Support search by group and variant names. Filter by appliance, category, tracking mode, pack status, and expiration. Sort by name, expiration, quantity, and appliance where appropriate. Make filters easy to clear.

## Expiration
Expiration dates are optional. Distinguish expired items from items approaching expiration. Items without dates are not expired. Use consistent local-date interpretation to avoid timezone-related off-by-one errors. Preserve each variant's own date.

## Authentication
Do not display private dashboard content before successful authentication. API data must come from the authenticated backend.

## Updates
Refresh totals, dashboard cards, group contents, search results, appliance views, and expiration summaries consistently after every successful mutation.

## Tests
Test filters, grouped totals, missing dates, expiration boundaries, and updates after changes.