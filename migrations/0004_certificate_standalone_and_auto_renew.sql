ALTER TABLE certificate_orders
    DROP CONSTRAINT IF EXISTS certificate_orders_site_id_fkey;

ALTER TABLE certificate_orders
    ALTER COLUMN site_id DROP NOT NULL;

ALTER TABLE certificate_orders
    ADD CONSTRAINT certificate_orders_site_id_fkey
    FOREIGN KEY (site_id) REFERENCES sites(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_certificates_auto_renew
    ON certificates (status, not_after)
    WHERE not_after IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_certificate_orders_renewal_guard
    ON certificate_orders (certificate_id, order_type, order_status)
    WHERE certificate_id IS NOT NULL;
