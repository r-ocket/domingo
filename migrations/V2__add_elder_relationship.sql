-- Add relationship field to elders table
-- This field stores the relationship of the elder to their caregiver (e.g., "madre", "padre", "abuela")
ALTER TABLE elders ADD COLUMN relationship VARCHAR(100) DEFAULT 'familiar';

