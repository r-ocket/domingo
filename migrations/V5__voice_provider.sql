-- Add voice_provider column to track which AI system handled each call
-- Possible values: 'openai_realtime', 'elevenlabs'
ALTER TABLE call_sessions ADD COLUMN voice_provider TEXT NOT NULL DEFAULT 'openai_realtime';

