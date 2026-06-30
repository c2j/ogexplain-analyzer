UPDATE rental r
SET return_date = (
  SELECT MAX(payment_date)
  FROM payment p
  WHERE p.rental_id = r.rental_id
)
WHERE r.return_date IS NULL
