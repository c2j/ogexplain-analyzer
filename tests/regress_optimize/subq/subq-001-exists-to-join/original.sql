SELECT f.film_id, f.title,
       (SELECT COUNT(*) FROM film_actor fa WHERE fa.film_id = f.film_id) AS actor_cnt
FROM film f
WHERE f.rating = 'PG'
LIMIT 20
