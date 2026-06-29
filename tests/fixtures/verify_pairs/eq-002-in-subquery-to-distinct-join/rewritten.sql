-- Case: eq-002-in-subquery-to-distinct-join
-- Role: rewritten
-- Trigger diagnostic: REW-001 (large IN list not rewritten)
-- Schema required: schemas/schema_pk.json
-- Expected verify result: Equivalent

SELECT DISTINCT o.id
FROM orders o
JOIN users u ON u.id = o.uid;
