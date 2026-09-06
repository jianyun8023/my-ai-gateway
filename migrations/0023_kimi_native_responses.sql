-- Issue #157: Kimi Code officially serves OpenAI Responses natively at
-- /v1/responses (verified 2026-09-06 against api.kimi.com/coding: reasoning
-- items, streaming function_call events, cached-token usage).  The embedded
-- kimi_responses_adapter is retired, so Sources on the kimi_code preset move
-- to preset version 4 whose Responses chain is native.  Per-model capability
-- rows keep their confirmed status; only the protocol chain changes.
--
-- The migration scripts are replayed on every startup, so all statements run
-- only when version 23 is first registered.
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM gateway_schema_migrations WHERE version = 23
  ) THEN
    INSERT INTO gateway_schema_migrations (version, name)
    VALUES (23, 'kimi_native_responses');

    UPDATE gateway_schema_metadata
    SET schema_version = GREATEST(schema_version, 23),
        migration_version = GREATEST(migration_version, 23),
        updated_at = NOW()
    WHERE singleton = TRUE;

    -- Register the kimi_code@4 preset inside the migration: the
    -- sources.provider_preset_version foreign key needs the row before the
    -- Source update below, and install_builtin_presets runs later in boot.
    INSERT INTO provider_presets (id, version, display_name, definition)
    VALUES ('kimi_code', 4, 'Kimi Code', '{"credential_header":{"header":"authorization","prefix":"Bearer "},"default_base_url":"https://api.kimi.com/coding","default_headers":{"accept":"application/json","content-type":"application/json"},"discovery":{"reason":"Kimi Code does not document an authenticated model-list endpoint; use the versioned ModelPreset catalog and user confirmation","support":"unsupported"},"protocols":{"anthropic_messages":{"connection_test":{"body":{"max_tokens":1,"messages":[{"content":"ping","role":"user"}],"model":"{{model}}","stream":false},"default_model":"k3","method":"post"},"default_capabilities":{"streaming":"supported","structured_output":"unknown","thinking":"supported","tools":"supported","usage":"supported","web_search":"unknown"},"endpoint":"/v1/messages","headers":{"anthropic-version":"2023-06-01"},"mode":"native"},"openai_chat_completions":{"connection_test":{"body":{"max_tokens":1,"messages":[{"content":"ping","role":"user"}],"model":"{{model}}","stream":false},"default_model":"k3","method":"post"},"default_capabilities":{"streaming":"supported","structured_output":"unknown","thinking":"supported","tools":"supported","usage":"supported","web_search":"unknown"},"endpoint":"/v1/chat/completions","headers":{},"mode":"native"},"openai_responses":{"connection_test":{"body":{"input":"ping","max_output_tokens":1,"model":"{{model}}","stream":false},"default_model":"k3","method":"post"},"default_capabilities":{"streaming":"supported","structured_output":"unknown","thinking":"supported","tool_streaming":"supported","tools":"supported","usage":"supported","web_search":"supported","web_search_citations":"unknown","web_search_sources":"unknown"},"endpoint":"/v1/responses","headers":{},"mode":"native"}},"schema_version":1}'::jsonb)
    ON CONFLICT (id, version) DO NOTHING;

    -- Rebase existing kimi_code Sources onto the v4 snapshot.  Per-Source
    -- endpoint overrides for chat completions/messages are preserved; only
    -- the Responses entry moves from the adapter target (/v1/messages) to the
    -- native /v1/responses path, and its capability chain becomes native.
    UPDATE sources
    SET provider_preset_version = 4,
        provider_preset_snapshot = (SELECT definition FROM provider_presets WHERE id = 'kimi_code' AND version = 4),
        endpoints = jsonb_set(endpoints, '{openai_responses}', '"/v1/responses"', true),
        protocol_capabilities = jsonb_set(
          protocol_capabilities,
          '{openai_responses}',
          '{"adapter":null,"features":{"streaming":"supported","structured_output":"unknown","thinking":"supported","tool_streaming":"supported","tools":"supported","usage":"supported","web_search":"supported","web_search_citations":"unknown","web_search_sources":"unknown"},"mode":"native","source_protocol":null}'::jsonb,
          true
        ),
        updated_at = NOW()
    WHERE provider_preset_id = 'kimi_code';

    -- Convert the per-model Responses capability rows from the retired
    -- adapter chain to native.  Status (confirmed/pending/unavailable) and
    -- the confirmed feature set stay untouched.
    UPDATE source_model_capabilities
    SET mode = 'native',
        source_protocol = NULL,
        adapter = NULL,
        updated_at = NOW()
    WHERE adapter = 'kimi_responses_adapter'
      AND protocol = 'openai_responses';
  END IF;
END $$;
