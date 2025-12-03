-- Caregiver profile enhancements
ALTER TABLE caregivers 
ADD COLUMN phone VARCHAR(50),
ADD COLUMN notes TEXT;

-- Caregiver-Elder relationship with context notes
CREATE TABLE caregiver_elder_relationships (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    caregiver_id UUID NOT NULL REFERENCES caregivers(id) ON DELETE CASCADE,
    elder_id UUID NOT NULL REFERENCES elders(id) ON DELETE CASCADE,
    relationship VARCHAR(100) NOT NULL DEFAULT 'Cuidador',
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(caregiver_id, elder_id)
);

CREATE INDEX idx_caregiver_elder_rel_caregiver ON caregiver_elder_relationships(caregiver_id);
CREATE INDEX idx_caregiver_elder_rel_elder ON caregiver_elder_relationships(elder_id);

-- Populate relationships from existing elder assignments
INSERT INTO caregiver_elder_relationships (caregiver_id, elder_id, relationship)
SELECT caregiver_id, id, 'Cuidador'
FROM elders
WHERE caregiver_id IS NOT NULL
ON CONFLICT DO NOTHING;

-- Location types: destination (places to go) vs common_spot (places they might be)
ALTER TABLE locations 
ADD COLUMN location_type VARCHAR(20) NOT NULL DEFAULT 'destination',
ADD COLUMN tags TEXT[] DEFAULT '{}';

-- Add check constraint for location_type
ALTER TABLE locations 
ADD CONSTRAINT chk_location_type CHECK (location_type IN ('destination', 'common_spot'));

-- Index for filtering by type
CREATE INDEX idx_locations_type ON locations(elder_id, location_type);

-- Add comments
COMMENT ON COLUMN caregivers.notes IS 'General preferences/notes about the caregiver';
COMMENT ON TABLE caregiver_elder_relationships IS 'Relationship context between caregiver and elder for AI prompts';
COMMENT ON COLUMN locations.location_type IS 'destination = places to go, common_spot = places elder might be when calling';
COMMENT ON COLUMN locations.tags IS 'Searchable labels for the location';

