-- Case: eq-003-add-explicit-cast
-- Role: original
-- Trigger diagnostic: TYPE-001 (implicit type coercion)
-- Schema required: schemas/schema_pk.json
-- Expected verify result: Equivalent

SELECT id, amount
FROM orders
WHERE amount + 0 = 100;
