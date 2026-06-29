-- Case: ne-002-in-subquery-to-join-no-distinct-no-pk
-- Role: rewritten (unsound — missing DISTINCT)
-- Trigger diagnostic: REW-001 (large IN list not rewritten)
-- Schema required: schemas/schema_nopk.json
-- Expected verify result: NotEquivalent

SELECT o.id
FROM orders o
JOIN users u ON u.id = o.uid;
