-- change default voice provider for new call_sessions rows
ALTER TABLE call_sessions ALTER COLUMN voice_provider SET DEFAULT 'gemini_live';


