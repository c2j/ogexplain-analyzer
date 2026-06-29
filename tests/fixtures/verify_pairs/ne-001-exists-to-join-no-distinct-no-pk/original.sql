-- Case: ne-001-exists-to-join-no-distinct-no-pk
-- Role: original
-- Trigger diagnostic: SUBQ-001 (subquery not pulled up)
-- Schema required: schemas/schema_nopk.json
-- Expected verify result: NotEquivalent

SELECT o.id
FROM orders o
WHERE EXISTS (
    SELECT 1 FROM users u WHERE u.id = o.uid
);
