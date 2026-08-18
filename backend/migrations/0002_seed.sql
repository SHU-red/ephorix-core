-- POC seed data. Fixed dev tokens for the header-auth middleware.
-- In production these tokens are per-user secrets; here they are documented
-- dev credentials so the POC works out of the box.

INSERT INTO users (id, token, display_name) VALUES
    ('00000000-0000-0000-0000-000000000001', 'ephorix-dev-1', 'Leonidas'),
    ('00000000-0000-0000-0000-000000000002', 'ephorix-dev-2', 'Gorgo')
ON CONFLICT (token) DO NOTHING;

-- Palette is strictly black/red; each type gets a black or red accent.
INSERT INTO agoge_types (name, color_code, icon) VALUES
    ('Strength', '#E53935', 'dumbbell'),
    ('Cycling',  '#000000', 'bicycle'),
    ('Climbing', '#B71C1C', 'mountain'),
    ('Running',  '#7B0000', 'runner'),
    ('Rowing',   '#8B0000', 'rowing')
ON CONFLICT DO NOTHING;
