import { RuleTester } from 'eslint';
import { describe, it } from 'vitest';
import architecture from './eslint.architecture.js';

RuleTester.describe = describe;
RuleTester.it = it;
new RuleTester().run('module-boundaries', architecture.rules['module-boundaries'], {
  valid: [
    { filename: '/app/src/pages/Page.tsx', code: "import { Screen } from '@/features/control-plane/Screen';" },
    { filename: '/app/src/features/usage/View.tsx', code: "import { Button } from '@/components/ui/Button';" },
    { filename: '/app/src/gateway-usage/client.ts', code: "import { AdminClient } from '../admin-api/client';" },
  ],
  invalid: [
    { filename: '/app/src/lib/util.ts', code: "export { default } from '../App';", errors: [{ messageId: 'boundary' }] },
    { filename: '/app/src/admin-api/client.ts', code: "import { Screen } from '@/features/control-plane/Screen';", errors: [{ messageId: 'boundary' }] },
    { filename: '/app/src/components/ui/Card.tsx', code: "import { Resources } from '../../admin-api/resources';", errors: [{ messageId: 'boundary' }] },
    { filename: '/app/src/features/usage/model.ts', code: "export { Page } from '../../pages/Page';", errors: [{ messageId: 'boundary' }] },
    { filename: '/app/src/features/usage/Screen.tsx', code: "const page = import('../control-plane/Screen');", errors: [{ messageId: 'boundary' }] },
  ],
});
