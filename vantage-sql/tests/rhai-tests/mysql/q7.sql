SELECT
  `o`.`id` AS `order_id`,
  `c`.`name` AS `client`,
  `o`.`created_at` AS `placed`,
  COUNT(`ol`.`id`) AS `lines`,
  SUM((`ol`.`quantity` * `ol`.`price`)) AS `order_total`,
  SUM(`ol`.`quantity`) AS `total_items`
FROM
  `client_order` AS `o`
  INNER JOIN `client` AS `c` ON `c`.`id` = `o`.`client_id`
  LEFT JOIN `order_line` AS `ol` ON `ol`.`order_id` = `o`.`id`
GROUP BY
  `o`.`id`,
  `c`.`name`,
  `o`.`created_at`
ORDER BY
  `o`.`created_at`