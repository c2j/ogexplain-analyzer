-- Case: eq-001-exists-to-distinct-join
-- Role: rewritten
-- Trigger diagnostic: SUBQ-001 (subquery not pulled up)
-- Schema required: schemas/schema_pk.json
-- Expected verify result: Equivalent

SELECT DISTINCT o.id
FROM orders o
JOIN users u ON u.id = o.uid;
