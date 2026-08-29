-- v14: explicit web-editable ordering for Agoge types.
-- sort_order is a plain INT rank (0 = first). The web UI reorders via
-- POST /api/v1/agoge-types/reorder; create() appends new types at MAX+1.
ALTER TABLE agoge_types
    ADD COLUMN sort_order INT NOT NULL DEFAULT 0;

-- Backfill existing rows in creation order (stable tiebreak by id).
UPDATE agoge_types t
SET sort_order = sub.rn
FROM (
    SELECT id, row_number() OVER (ORDER BY created_at, id) AS rn
    FROM agoge_types
) sub
WHERE t.id = sub.id;
