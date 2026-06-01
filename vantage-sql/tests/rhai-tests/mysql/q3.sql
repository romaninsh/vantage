SELECT
  DATE_FORMAT(`o`.`created_at`, '%Y-%m') AS `month`,
  COUNT(DISTINCT `o`.`id`) AS `orders`,
  COALESCE(SUM((`ol`.`quantity` * `ol`.`price`)), 0) AS `revenue`
FROM
  `client_order` AS `o`
  LEFT JOIN `order_line` AS `ol` ON `ol`.`order_id` = `o`.`id`
GROUP BY
  DATE_FORMAT(`o`.`created_at`, '%Y-%m')
ORDER BY
  MONTH