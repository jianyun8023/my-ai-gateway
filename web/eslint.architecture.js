import path from 'node:path';

const uiLayers = new Set(['pages', 'features', 'components', 'hooks', 'stores']);
const roots = new Set(['App', 'Root', 'main']);

function forbiddenImport(from, to) {
  const [sourceLayer] = from.split('/');
  const [targetLayer] = to.split('/');
  if (sourceLayer !== 'pages' && sourceLayer !== 'App.tsx' && sourceLayer !== 'Root.tsx' && sourceLayer !== 'main.tsx') {
    if (targetLayer === 'pages' || roots.has(to.replace(/\.[cm]?[jt]sx?$/, ''))) return true;
  }
  if (['admin-api', 'gateway-usage', 'lib', 'utils', 'i18n'].includes(sourceLayer)) return uiLayers.has(targetLayer);
  if (sourceLayer === 'hooks') return ['pages', 'features', 'components'].includes(targetLayer);
  if (from.startsWith('components/ui/')) return ['admin-api', 'gateway-usage', 'features', 'pages'].includes(targetLayer);
  if (sourceLayer === 'features' && targetLayer === 'features') return from.split('/')[1] !== to.split('/')[1];
  return false;
}

export default {
  rules: {
    'module-boundaries': {
      meta: {
        type: 'problem',
        schema: [],
        messages: { boundary: '{{from}} must not depend on {{to}}. Move shared code to its owning lower layer.' },
      },
      create(context) {
        const filename = context.filename.replaceAll(path.sep, '/');
        const marker = filename.lastIndexOf('/src/');
        if (marker === -1) return {};
        const from = filename.slice(marker + 5);
        const check = (node) => {
          if (!node || typeof node.value !== 'string') return;
          const specifier = node.value;
          const to = specifier.startsWith('@/') ? specifier.slice(2)
            : specifier.startsWith('.') ? path.posix.normalize(path.posix.join(path.posix.dirname(from), specifier)) : undefined;
          if (to && forbiddenImport(from, to)) context.report({ node, messageId: 'boundary', data: { from, to } });
        };
        return {
          ImportDeclaration: node => check(node.source),
          ExportNamedDeclaration: node => check(node.source),
          ExportAllDeclaration: node => check(node.source),
          ImportExpression: node => check(node.source),
        };
      },
    },
  },
};
