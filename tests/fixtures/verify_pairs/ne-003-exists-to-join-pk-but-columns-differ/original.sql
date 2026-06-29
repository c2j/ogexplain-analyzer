-- Case: ne-003-exists-to-join-pk-but-columns-differ
-- Role: original
-- Trigger diagnostic: SUBQ-001 (subquery not pulled up)
-- Schema required: schemas/schema_mixed.json
-- Expected verify result: NotEquivalent

SELECT o.id, o.amount
FROM orders o
WHERE EXISTS (
    SELECT 1 FROM users u WHERE u.id = o.uid
);
