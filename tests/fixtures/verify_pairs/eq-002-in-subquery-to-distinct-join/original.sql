-- Case: eq-002-in-subquery-to-distinct-join
-- Role: original
-- Trigger diagnostic: REW-001 (large IN list not rewritten)
-- Schema required: schemas/schema_pk.json
-- Expected verify result: Equivalent

SELECT o.id
FROM orders o
WHERE o.uid IN (SELECT id FROM users);
