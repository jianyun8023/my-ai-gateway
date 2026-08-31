import { describe, expect, it } from 'vitest';
import { createMetadataDraft, metadataFromDraft } from './ModelMetadataEditor';

describe('model metadata editing', () => {
  it('preserves explicit unknown values instead of guessing support', () => {
    const draft = createMetadataDraft({ tools: 'unknown', context_window: null });
    expect(draft.tools).toBe('unknown');
    expect(draft.context_window).toBe('');
    expect(metadataFromDraft(draft, ['tools'])).toEqual({ tools: 'unknown' });
  });

  it('serializes only selected dirty fields with typed values', () => {
    const draft = createMetadataDraft();
    draft.context_window = '128000';
    draft.input_modalities = 'text, image';
    draft.web_search = 'unsupported';

    expect(metadataFromDraft(draft, ['context_window', 'input_modalities', 'web_search'])).toEqual({
      context_window: 128000,
      input_modalities: ['text', 'image'],
      web_search: 'unsupported',
    });
  });
});
