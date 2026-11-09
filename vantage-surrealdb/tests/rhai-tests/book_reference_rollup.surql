SELECT name, math::max(<~(order FIELD client).total) AS biggest_order FROM client
