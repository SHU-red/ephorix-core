-- v8: macro breakdown for nutrition entries. A "meal" is modeled as
-- kind='food' with meal_type set, so the kind CHECK is untouched.
ALTER TABLE nutrition_log
    ADD COLUMN protein REAL,
    ADD COLUMN carbs REAL,
    ADD COLUMN fat REAL,
    ADD COLUMN meal_type TEXT;
