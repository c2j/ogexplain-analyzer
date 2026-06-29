UPDATE film f
SET rental_rate = rental_rate * 1.1
WHERE EXISTS (
    SELECT 1 FROM film_actor fa
    WHERE fa.film_id = f.film_id
      AND fa.actor_id < 5
)
