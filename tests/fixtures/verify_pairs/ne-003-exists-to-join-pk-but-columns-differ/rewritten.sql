-- Case: ne-003-exists-to-join-pk-but-columns-differ
-- Role: rewritten (intentionally wrong — projection changed)
-- Trigger diagnostic: SUBQ-001 (subquery not pulled up)
-- Schema required: schemas/schema_mixed.json
-- Expected verify result: NotEquivalent

SELECT o.id
FROM orders o
JOIN users u ON u.id = o.uid;
