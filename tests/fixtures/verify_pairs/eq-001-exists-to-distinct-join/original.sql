-- Case: eq-001-exists-to-distinct-join
-- Role: original
-- Trigger diagnostic: SUBQ-001 (subquery not pulled up)
-- Schema required: schemas/schema_pk.json
-- Expected verify result: Equivalent

SELECT o.id
FROM orders o
WHERE EXISTS (
    SELECT 1 FROM users u WHERE u.id = o.uid
);
