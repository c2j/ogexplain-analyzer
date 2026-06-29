-- Case: ne-001-exists-to-join-no-distinct-no-pk
-- Role: rewritten (unsound — missing DISTINCT)
-- Trigger diagnostic: SUBQ-001 (subquery not pulled up)
-- Schema required: schemas/schema_nopk.json
-- Expected verify result: NotEquivalent

SELECT o.id
FROM orders o
JOIN users u ON u.id = o.uid;
