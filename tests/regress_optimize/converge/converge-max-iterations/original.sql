SELECT * FROM film WHERE film_id IN (SELECT film_id FROM film_actor WHERE actor_id = 1)
