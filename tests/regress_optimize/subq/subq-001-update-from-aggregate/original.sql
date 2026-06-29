UPDATE customer c
SET active = 0
FROM (
    SELECT customer_id FROM payment
    GROUP BY customer_id
    HAVING SUM(amount) > 100
) sub
WHERE c.customer_id = sub.customer_id
