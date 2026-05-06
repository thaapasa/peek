-- example.sql — showcase of common SQL constructs
-- Dialect: roughly PostgreSQL, but kept portable where possible.
-- Use this file to verify syntax highlighting, comment handling,
-- string escaping, and structural keywords in `peek`.

/* ------------------------------------------------------------------
 * Schema setup
 *   - Block comments survive across multiple lines.
 *   - DDL keywords, types, and constraints exercise highlighting.
 * ------------------------------------------------------------------ */

DROP TABLE IF EXISTS order_items CASCADE;
DROP TABLE IF EXISTS orders      CASCADE;
DROP TABLE IF EXISTS customers   CASCADE;
DROP TABLE IF EXISTS products    CASCADE;

CREATE TABLE customers (
    id           BIGSERIAL PRIMARY KEY,
    email        TEXT       NOT NULL UNIQUE,
    display_name TEXT       NOT NULL,
    country      CHAR(2)    NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- soft-delete column; NULL means active
    deleted_at   TIMESTAMPTZ
);

CREATE TABLE products (
    id          BIGSERIAL PRIMARY KEY,
    sku         TEXT      NOT NULL UNIQUE,
    name        TEXT      NOT NULL,
    price_cents INTEGER   NOT NULL CHECK (price_cents >= 0),
    tags        TEXT[]    NOT NULL DEFAULT '{}'
);

CREATE TABLE orders (
    id           BIGSERIAL PRIMARY KEY,
    customer_id  BIGINT    NOT NULL REFERENCES customers(id),
    placed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    status       TEXT      NOT NULL CHECK (status IN ('pending','paid','shipped','cancelled'))
);

CREATE TABLE order_items (
    order_id    BIGINT  NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    product_id  BIGINT  NOT NULL REFERENCES products(id),
    quantity    INTEGER NOT NULL CHECK (quantity > 0),
    unit_cents  INTEGER NOT NULL,
    PRIMARY KEY (order_id, product_id)
);

CREATE INDEX idx_orders_customer ON orders (customer_id);
CREATE INDEX idx_orders_placed   ON orders (placed_at DESC);

/* ------------------------------------------------------------------
 * Sample data
 *   Strings with embedded quotes use the SQL '' escape.
 *   E'\n' is a Postgres extension for C-style escapes.
 * ------------------------------------------------------------------ */

INSERT INTO customers (email, display_name, country) VALUES
    ('ada@example.com',  'Ada Lovelace',     'GB'),
    ('alan@example.com', 'Alan Turing',      'GB'),
    ('grace@example.com','Grace ''Amazing'' Hopper', 'US'),
    ('linus@example.com','Linus Torvalds',   'FI');

INSERT INTO products (sku, name, price_cents, tags) VALUES
    ('BK-001', 'The Mythical Man-Month',     2499, ARRAY['book','classic']),
    ('BK-002', 'Structure & Interpretation', 3999, ARRAY['book','classic','cs']),
    ('MG-001', 'Coffee Mug',                  990, ARRAY['merch']),
    ('TS-001', 'T-Shirt — "Hello, World!"',  1799, ARRAY['merch','apparel']);

-- Single-row insert with a returning clause
INSERT INTO orders (customer_id, status)
VALUES (1, 'paid')
RETURNING id, placed_at;

/* ------------------------------------------------------------------
 * Queries
 * ------------------------------------------------------------------ */

-- 1. Plain SELECT with WHERE / ORDER BY / LIMIT
SELECT id, email, country
FROM   customers
WHERE  deleted_at IS NULL
  AND  country IN ('GB', 'US')
ORDER  BY created_at DESC
LIMIT  10;

-- 2. Multi-table JOIN with aggregation and HAVING
SELECT  c.id                               AS customer_id,
        c.display_name,
        COUNT(DISTINCT o.id)               AS order_count,
        SUM(oi.quantity * oi.unit_cents)   AS total_cents
FROM    customers       c
JOIN    orders          o  ON o.customer_id = c.id
JOIN    order_items     oi ON oi.order_id   = o.id
WHERE   o.status <> 'cancelled'
GROUP   BY c.id, c.display_name
HAVING  SUM(oi.quantity * oi.unit_cents) > 1000
ORDER   BY total_cents DESC;

-- 3. LEFT JOIN to find customers without orders
SELECT   c.id, c.email
FROM     customers c
LEFT JOIN orders   o ON o.customer_id = c.id
WHERE    o.id IS NULL;

-- 4. Common Table Expression (CTE) with window functions
WITH monthly_revenue AS (
    SELECT date_trunc('month', o.placed_at)        AS month,
           SUM(oi.quantity * oi.unit_cents)::bigint AS cents
    FROM   orders      o
    JOIN   order_items oi ON oi.order_id = o.id
    WHERE  o.status = 'paid'
    GROUP  BY 1
),
ranked AS (
    SELECT month,
           cents,
           LAG(cents)  OVER (ORDER BY month) AS prev_cents,
           RANK()      OVER (ORDER BY cents DESC) AS rev_rank
    FROM   monthly_revenue
)
SELECT month,
       cents / 100.0                                AS revenue_eur,
       (cents - COALESCE(prev_cents, 0)) / 100.0    AS delta_eur,
       rev_rank
FROM   ranked
ORDER  BY month;

-- 5. Recursive CTE: walk an org chart (toy example)
WITH RECURSIVE descendants(id, parent_id, depth) AS (
    SELECT id, parent_id, 0
    FROM   employees
    WHERE  id = 1                            -- root
    UNION ALL
    SELECT e.id, e.parent_id, d.depth + 1
    FROM   employees   e
    JOIN   descendants d ON e.parent_id = d.id
)
SELECT * FROM descendants ORDER BY depth, id;

-- 6. Subquery in SELECT list (correlated)
SELECT  c.id,
        c.display_name,
        (SELECT MAX(o.placed_at)
         FROM   orders o
         WHERE  o.customer_id = c.id) AS last_order_at
FROM    customers c;

-- 7. UPSERT (INSERT ... ON CONFLICT)
INSERT INTO products (sku, name, price_cents)
VALUES ('BK-001', 'The Mythical Man-Month (Anniversary Ed.)', 2999)
ON CONFLICT (sku) DO UPDATE
   SET name        = EXCLUDED.name,
       price_cents = EXCLUDED.price_cents;

-- 8. CASE expression + COALESCE
SELECT  o.id,
        COALESCE(c.display_name, '<deleted>') AS who,
        CASE
            WHEN o.status = 'paid'    THEN 'OK'
            WHEN o.status = 'pending' THEN 'WAIT'
            ELSE                          'BAD'
        END AS health
FROM    orders     o
LEFT    JOIN customers c ON c.id = o.customer_id;

-- 9. Set operations
SELECT email FROM customers WHERE country = 'GB'
UNION
SELECT email FROM customers WHERE country = 'US'
EXCEPT
SELECT email FROM customers WHERE deleted_at IS NOT NULL;

-- 10. Transactional update
BEGIN;
    UPDATE orders
       SET status = 'shipped'
     WHERE status = 'paid'
       AND placed_at < now() - INTERVAL '2 days';

    -- Sanity check: roll back if more than 1000 rows affected
    DO $$
    BEGIN
        IF (SELECT count(*) FROM orders WHERE status = 'shipped') > 1000 THEN
            RAISE EXCEPTION 'Too many shipments in one batch';
        END IF;
    END
    $$;
COMMIT;

-- 11. View definition
CREATE OR REPLACE VIEW v_customer_totals AS
SELECT  c.id,
        c.display_name,
        SUM(oi.quantity * oi.unit_cents) AS lifetime_cents
FROM    customers       c
LEFT    JOIN orders     o  ON o.customer_id = c.id
LEFT    JOIN order_items oi ON oi.order_id  = o.id
GROUP   BY c.id, c.display_name;

/* ------------------------------------------------------------------
 * PL/pgSQL function
 *   Stored procedure with declarations, control flow, cursors,
 *   exception handling, and dynamic SQL — exercises highlighting
 *   inside a $$-quoted body.
 * ------------------------------------------------------------------ */

CREATE OR REPLACE FUNCTION apply_loyalty_discount(
    p_customer_id BIGINT,
    p_threshold   INTEGER DEFAULT 10000,
    p_percent     NUMERIC DEFAULT 5.0
)
RETURNS TABLE(order_id BIGINT, old_cents BIGINT, new_cents BIGINT)
LANGUAGE plpgsql
AS $$
DECLARE
    v_lifetime_cents BIGINT;
    v_factor         NUMERIC := 1.0 - (p_percent / 100.0);
    v_row            RECORD;
    v_updated        INTEGER := 0;
BEGIN
    -- Guard: percent must be in (0, 100)
    IF p_percent <= 0 OR p_percent >= 100 THEN
        RAISE EXCEPTION 'Discount percent out of range: %', p_percent
            USING HINT = 'Pass a value strictly between 0 and 100.';
    END IF;

    -- Fetch lifetime spend; bail early if below threshold
    SELECT COALESCE(SUM(oi.quantity * oi.unit_cents), 0)
      INTO v_lifetime_cents
      FROM orders      o
      JOIN order_items oi ON oi.order_id = o.id
     WHERE o.customer_id = p_customer_id
       AND o.status      = 'paid';

    IF v_lifetime_cents < p_threshold * 100 THEN
        RAISE NOTICE 'Customer % under threshold (% cents)',
                     p_customer_id, v_lifetime_cents;
        RETURN;
    END IF;

    -- Walk pending orders, apply discount, emit before/after
    FOR v_row IN
        SELECT o.id   AS oid,
               SUM(oi.quantity * oi.unit_cents)::BIGINT AS cents
          FROM orders      o
          JOIN order_items oi ON oi.order_id = o.id
         WHERE o.customer_id = p_customer_id
           AND o.status      = 'pending'
         GROUP BY o.id
    LOOP
        UPDATE order_items
           SET unit_cents = (unit_cents * v_factor)::INTEGER
         WHERE order_items.order_id = v_row.oid;

        order_id  := v_row.oid;
        old_cents := v_row.cents;
        new_cents := (v_row.cents * v_factor)::BIGINT;
        v_updated := v_updated + 1;
        RETURN NEXT;
    END LOOP;

    RAISE NOTICE 'Discounted % order(s) for customer %', v_updated, p_customer_id;

EXCEPTION
    WHEN division_by_zero THEN
        RAISE WARNING 'Unexpected division by zero — falling back to no-op';
        RETURN;
    WHEN OTHERS THEN
        -- Re-raise with context; PL/pgSQL preserves SQLSTATE
        RAISE EXCEPTION 'apply_loyalty_discount failed: %', SQLERRM
            USING ERRCODE = SQLSTATE;
END;
$$;

-- Trigger function: stamp updated_at on row mutation
CREATE OR REPLACE FUNCTION trg_set_updated_at()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    NEW.updated_at := now();
    RETURN NEW;
END;
$$;

CREATE TRIGGER orders_set_updated_at
    BEFORE UPDATE ON orders
    FOR EACH ROW
    EXECUTE FUNCTION trg_set_updated_at();
