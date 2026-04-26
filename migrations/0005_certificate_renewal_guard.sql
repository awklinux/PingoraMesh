DROP INDEX IF EXISTS idx_certificate_orders_renewal_guard;

WITH ranked_active_renewals AS (
    SELECT
        id,
        row_number() OVER (
            PARTITION BY certificate_id
            ORDER BY created_at DESC, id DESC
        ) AS renewal_rank
    FROM certificate_orders
    WHERE
        certificate_id IS NOT NULL
        AND order_type = 'renew'
        AND order_status IN ('pending_dns_challenge', 'dns_challenge_presenting', 'dns_challenge_presented', 'issuing')
)
UPDATE certificate_orders AS orders
SET
    order_status = 'canceled',
    error_message = COALESCE(error_message, 'superseded by a newer active auto-renew order'),
    updated_at = now()
FROM ranked_active_renewals AS ranked
WHERE
    orders.id = ranked.id
    AND ranked.renewal_rank > 1;

CREATE UNIQUE INDEX IF NOT EXISTS idx_certificate_orders_one_active_renew
    ON certificate_orders (certificate_id)
    WHERE
        certificate_id IS NOT NULL
        AND order_type = 'renew'
        AND order_status IN ('pending_dns_challenge', 'dns_challenge_presenting', 'dns_challenge_presented', 'issuing');
