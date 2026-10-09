-- Issue #250: request records only knew the client-requested logical model and
-- the rewritten upstream model. Providers may silently report a different
-- model (version snapshot mapping, provider-side routing) or execute at a
-- different reasoning effort than requested, and the console could not surface
-- either drift.
--
-- Persist the upstream-reported identity per logical request:
--   response_model               -- model string reported by the upstream response
--   requested_reasoning_effort   -- reasoning effort carried by the client request
--   response_reasoning_effort    -- reasoning effort reported by the upstream response
-- NULL means the corresponding side did not carry the information.

INSERT INTO gateway_schema_migrations (version, name)
VALUES (28, 'usage_response_identity')
ON CONFLICT (version) DO NOTHING;

UPDATE gateway_schema_metadata
SET schema_version = GREATEST(schema_version, 28),
    migration_version = GREATEST(migration_version, 28),
    updated_at = NOW()
WHERE singleton = TRUE;

ALTER TABLE usage_events
  ADD COLUMN IF NOT EXISTS response_model TEXT;
ALTER TABLE usage_events
  ADD COLUMN IF NOT EXISTS requested_reasoning_effort TEXT;
ALTER TABLE usage_events
  ADD COLUMN IF NOT EXISTS response_reasoning_effort TEXT;

COMMENT ON COLUMN usage_events.response_model IS
  'Model string reported by the upstream response body (streamed or JSON); NULL when the provider did not report one.';
COMMENT ON COLUMN usage_events.requested_reasoning_effort IS
  'Reasoning effort carried by the client request (reasoning_effort / reasoning.effort / effort); NULL when absent.';
COMMENT ON COLUMN usage_events.response_reasoning_effort IS
  'Reasoning effort reported by the upstream response; NULL when the provider did not report one.';
