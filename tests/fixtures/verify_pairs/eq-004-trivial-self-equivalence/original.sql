-- Case: eq-004-trivial-self-equivalence
-- Role: original
-- Trigger diagnostic: none (smoke test)
-- Schema required: any (schemas/schema_pk.json is fine)
-- Expected verify result: Equivalent

SELECT id, name FROM users WHERE id = 1;
