WITH high_payers AS (
    SELECT customer_id, SUM(amount) AS total
    FROM payment
    GROUP BY customer_id
    HAVING SUM(amount) > 100
)
SELECT c.customer_id, c.first_name
FROM customer c
WHERE c.customer_id IN (SELECT customer_id FROM high_payers)
  AND EXISTS (SELECT 1 FROM rental r WHERE r.customer_id = c.customer_id);
