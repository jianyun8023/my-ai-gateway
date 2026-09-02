// @vitest-environment happy-dom
import { act, createElement } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { setTestLanguage } from '@/test/setup';
import {
  ModelMetadataFields,
  createMetadataDraft,
  metadataFromDraft,
} from './ModelMetadataEditor';

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

describe('model metadata field labels', () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(async () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    await setTestLanguage('zh');
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  const renderFields = (draft = createMetadataDraft()) => {
    act(() => {
      root.render(createElement(ModelMetadataFields, { draft, onChange: () => {} }));
    });
  };

  it('renders localized field labels with zh fixed', () => {
    renderFields();
    const text = container.textContent ?? '';
    expect(text).toContain('建议逻辑模型名');
    expect(text).toContain('显示名称');
    expect(text).toContain('上下文窗口');
    expect(text).toContain('最大输入 Token');
    expect(text).toContain('最大输出 Token');
    expect(text).toContain('输入模态');
    expect(text).toContain('输出模态');
    expect(text).toContain('结构化输出');
    expect(text).toContain('流式');
    expect(text).toContain('用量');
    // 功能能力专名保留英文原文
    expect(text).toContain('Tools');
    expect(text).toContain('Thinking');
    expect(text).toContain('Web Search');
  });

  it('localizes feature option display text while keeping enum values', () => {
    renderFields();
    const options = Array.from(container.querySelectorAll<HTMLOptionElement>('select option'));
    expect(options.map((option) => option.textContent)).toEqual(expect.arrayContaining(['未知', '支持', '不支持']));
    expect(options.map((option) => option.value)).toEqual(expect.arrayContaining(['unknown', 'supported', 'unsupported']));
  });

  it('keeps field source pills as raw enum values', () => {
    renderFields();
    expect(container.textContent).toContain('unknown');
  });
});
