-- DeepSeek Flash and the current Kimi Code model catalog accept image input.
-- Register immutable v2 ModelPreset rows, rebase the affected catalog records,
-- and repair the per-protocol feature maps consumed by the runtime matrix.
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM gateway_schema_migrations WHERE version = 27
  ) THEN
    INSERT INTO model_presets (
      id, version, canonical_model_id, aliases, metadata, field_sources
    ) VALUES
      (
        'deepseek-v4-flash', 2, 'deepseek-flash',
        '["deepseek-v4-flash","deepseek-v4-flash-vision-exp"]'::jsonb,
        '{"logical_model_name":null,"display_name":"DeepSeek V4.1 Flash","context_window":1048576,"max_input_tokens":null,"max_output_tokens":384000,"input_modalities":["text","image"],"output_modalities":["text"],"tools":"supported","thinking":"supported","web_search":"unknown","structured_output":"supported","streaming":"supported","usage":"supported"}'::jsonb,
        '{"logical_model_name":"unknown","display_name":"preset","context_window":"preset","max_input_tokens":"unknown","max_output_tokens":"preset","input_modalities":"preset","output_modalities":"preset","tools":"preset","thinking":"preset","web_search":"unknown","structured_output":"preset","streaming":"preset","usage":"preset"}'::jsonb
      ),
      (
        'kimi-k3', 2, 'k3', '[]'::jsonb,
        '{"logical_model_name":null,"display_name":"Kimi K3","context_window":1048576,"max_input_tokens":null,"max_output_tokens":null,"input_modalities":["text","image","video"],"output_modalities":["text"],"tools":"supported","thinking":"supported","web_search":"supported","structured_output":"unknown","streaming":"supported","usage":"supported"}'::jsonb,
        '{"logical_model_name":"unknown","display_name":"preset","context_window":"preset","max_input_tokens":"unknown","max_output_tokens":"unknown","input_modalities":"preset","output_modalities":"preset","tools":"preset","thinking":"preset","web_search":"preset","structured_output":"unknown","streaming":"preset","usage":"preset"}'::jsonb
      ),
      (
        'kimi-k3-256k', 2, 'k3-256k', '[]'::jsonb,
        '{"logical_model_name":null,"display_name":"Kimi K3 256K","context_window":262144,"max_input_tokens":null,"max_output_tokens":null,"input_modalities":["text","image"],"output_modalities":["text"],"tools":"supported","thinking":"supported","web_search":"supported","structured_output":"unknown","streaming":"supported","usage":"supported"}'::jsonb,
        '{"logical_model_name":"unknown","display_name":"preset","context_window":"preset","max_input_tokens":"unknown","max_output_tokens":"unknown","input_modalities":"preset","output_modalities":"preset","tools":"preset","thinking":"preset","web_search":"preset","structured_output":"unknown","streaming":"preset","usage":"preset"}'::jsonb
      ),
      (
        'kimi-for-coding', 2, 'kimi-for-coding', '[]'::jsonb,
        '{"logical_model_name":null,"display_name":"Kimi for Coding","context_window":1048576,"max_input_tokens":null,"max_output_tokens":null,"input_modalities":["text","image","video"],"output_modalities":["text"],"tools":"supported","thinking":"supported","web_search":"supported","structured_output":"unknown","streaming":"supported","usage":"supported"}'::jsonb,
        '{"logical_model_name":"unknown","display_name":"preset","context_window":"preset","max_input_tokens":"unknown","max_output_tokens":"unknown","input_modalities":"preset","output_modalities":"preset","tools":"preset","thinking":"preset","web_search":"preset","structured_output":"unknown","streaming":"preset","usage":"preset"}'::jsonb
      ),
      (
        'kimi-for-coding-highspeed', 2, 'kimi-for-coding-highspeed', '[]'::jsonb,
        '{"logical_model_name":null,"display_name":"Kimi for Coding HighSpeed","context_window":262144,"max_input_tokens":null,"max_output_tokens":null,"input_modalities":["text","image","video"],"output_modalities":["text"],"tools":"supported","thinking":"supported","web_search":"supported","structured_output":"unknown","streaming":"supported","usage":"supported"}'::jsonb,
        '{"logical_model_name":"unknown","display_name":"preset","context_window":"preset","max_input_tokens":"unknown","max_output_tokens":"unknown","input_modalities":"preset","output_modalities":"preset","tools":"preset","thinking":"preset","web_search":"preset","structured_output":"unknown","streaming":"preset","usage":"preset"}'::jsonb
      )
    ON CONFLICT (id, version) DO NOTHING;

    -- Rebase discovered SourceModels while preserving explicit user metadata.
    UPDATE source_models AS model
    SET metadata = model.metadata || jsonb_build_object(
          'display_name', CASE WHEN model.field_sources->>'display_name' = 'user'
            THEN model.metadata->'display_name' ELSE preset.metadata->'display_name' END,
          'context_window', CASE WHEN model.field_sources->>'context_window' = 'user'
            THEN model.metadata->'context_window' ELSE preset.metadata->'context_window' END,
          'input_modalities', CASE WHEN model.field_sources->>'input_modalities' = 'user'
            THEN model.metadata->'input_modalities' ELSE preset.metadata->'input_modalities' END
        ),
        field_sources = model.field_sources || jsonb_build_object(
          'display_name', CASE WHEN model.field_sources->>'display_name' = 'user'
            THEN model.field_sources->'display_name' ELSE '"preset"'::jsonb END,
          'context_window', CASE WHEN model.field_sources->>'context_window' = 'user'
            THEN model.field_sources->'context_window' ELSE '"preset"'::jsonb END,
          'input_modalities', CASE WHEN model.field_sources->>'input_modalities' = 'user'
            THEN model.field_sources->'input_modalities' ELSE '"preset"'::jsonb END
        ),
        matched_model_preset_id = preset.id,
        matched_model_preset_version = preset.version,
        updated_at = NOW()
    FROM sources AS source, model_presets AS preset
    WHERE model.source_id = source.id
      AND preset.version = 2
      AND preset.id = CASE
        WHEN source.provider_preset_id = 'deepseek'
          AND model.upstream_model_id IN (
            'deepseek-flash', 'deepseek-v4-flash', 'deepseek-v4-flash-vision-exp'
          ) THEN 'deepseek-v4-flash'
        WHEN source.provider_preset_id = 'kimi_code'
          AND model.upstream_model_id = 'k3' THEN 'kimi-k3'
        WHEN source.provider_preset_id = 'kimi_code'
          AND model.upstream_model_id = 'k3-256k' THEN 'kimi-k3-256k'
        WHEN source.provider_preset_id = 'kimi_code'
          AND model.upstream_model_id = 'kimi-for-coding' THEN 'kimi-for-coding'
        WHEN source.provider_preset_id = 'kimi_code'
          AND model.upstream_model_id = 'kimi-for-coding-highspeed'
          THEN 'kimi-for-coding-highspeed'
        ELSE NULL
      END;

    -- LogicalModel metadata is also user-editable; only preset-owned fields
    -- are refreshed while the preset reference moves to v2.
    UPDATE logical_models AS model
    SET metadata = model.metadata || jsonb_build_object(
          'display_name', CASE WHEN model.field_sources->>'display_name' = 'user'
            THEN model.metadata->'display_name' ELSE preset.metadata->'display_name' END,
          'context_window', CASE WHEN model.field_sources->>'context_window' = 'user'
            THEN model.metadata->'context_window' ELSE preset.metadata->'context_window' END,
          'input_modalities', CASE WHEN model.field_sources->>'input_modalities' = 'user'
            THEN model.metadata->'input_modalities' ELSE preset.metadata->'input_modalities' END
        ),
        field_sources = model.field_sources || jsonb_build_object(
          'display_name', CASE WHEN model.field_sources->>'display_name' = 'user'
            THEN model.field_sources->'display_name' ELSE '"preset"'::jsonb END,
          'context_window', CASE WHEN model.field_sources->>'context_window' = 'user'
            THEN model.field_sources->'context_window' ELSE '"preset"'::jsonb END,
          'input_modalities', CASE WHEN model.field_sources->>'input_modalities' = 'user'
            THEN model.field_sources->'input_modalities' ELSE '"preset"'::jsonb END
        ),
        model_preset_id = preset.id,
        model_preset_version = preset.version,
        updated_at = NOW()
    FROM model_presets AS preset
    WHERE preset.version = 2
      AND preset.id = CASE
        WHEN model.public_name IN (
          'deepseek-flash', 'deepseek-v4-flash', 'deepseek-v4-flash-vision-exp'
        ) THEN 'deepseek-v4-flash'
        WHEN model.public_name = 'k3' THEN 'kimi-k3'
        WHEN model.public_name = 'k3-256k' THEN 'kimi-k3-256k'
        WHEN model.public_name = 'kimi-for-coding' THEN 'kimi-for-coding'
        WHEN model.public_name = 'kimi-for-coding-highspeed'
          THEN 'kimi-for-coding-highspeed'
        ELSE NULL
      END;

    UPDATE source_model_capabilities AS capability
    SET feature_capabilities = jsonb_set(
          capability.feature_capabilities,
          '{vision}',
          '"supported"'::jsonb,
          TRUE
        ),
        updated_at = NOW()
    FROM sources AS source
    WHERE capability.source_id = source.id
      AND (
        (
          source.provider_preset_id = 'deepseek'
          AND capability.upstream_model_id IN (
            'deepseek-flash', 'deepseek-v4-flash', 'deepseek-v4-flash-vision-exp'
          )
        )
        OR
        (
          source.provider_preset_id = 'kimi_code'
          AND capability.upstream_model_id IN (
            'k3', 'k3-256k', 'kimi-for-coding', 'kimi-for-coding-highspeed'
          )
        )
      );

    INSERT INTO gateway_schema_migrations (version, name)
    VALUES (27, 'multimodal_model_capabilities');

    UPDATE gateway_schema_metadata
    SET schema_version = GREATEST(schema_version, 27),
        migration_version = GREATEST(migration_version, 27),
        updated_at = NOW()
    WHERE singleton = TRUE;
  END IF;
END $$;
