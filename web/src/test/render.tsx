import { createRoot as createDOMRoot, type Root } from 'react-dom/client';
import { ConsoleProvider } from '@/components/ui/ConsoleProvider';
import './setup';

// Component tests use the same provider/overlay stack as production. Mantine's
// test environment skips Portals/transitions; real-browser evidence covers those.
export function createRoot(container: Parameters<typeof createDOMRoot>[0]): Root {
  const root = createDOMRoot(container);
  return {
    render: (children) => root.render(<ConsoleProvider env="test">{children}</ConsoleProvider>),
    unmount: () => root.unmount(),
  };
}
