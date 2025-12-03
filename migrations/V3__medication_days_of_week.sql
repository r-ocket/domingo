-- Add days_of_week column to medication_schedules
-- Uses integer bitmask: Mon=1, Tue=2, Wed=4, Thu=8, Fri=16, Sat=32, Sun=64
-- Value of 127 means all days (daily), 31 means weekdays (Mon-Fri), 96 means weekends (Sat-Sun)

ALTER TABLE medication_schedules 
ADD COLUMN days_of_week INTEGER NOT NULL DEFAULT 127;

-- Migrate existing days_pattern data to days_of_week
UPDATE medication_schedules 
SET days_of_week = CASE 
    WHEN days_pattern = 'daily' THEN 127
    WHEN days_pattern = 'weekdays' THEN 31
    WHEN days_pattern = 'weekends' THEN 96
    ELSE 127  -- default to daily for any other pattern
END;

-- Add comment explaining the bitmask
COMMENT ON COLUMN medication_schedules.days_of_week IS 'Bitmask for days: Mon=1, Tue=2, Wed=4, Thu=8, Fri=16, Sat=32, Sun=64. 127=daily, 31=weekdays, 96=weekends';

