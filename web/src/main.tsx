import { StrictMode, useEffect } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import faviconUrl from './assets/gateway-icon.svg';
import './styles/reset.scss';
import './styles/variables.scss';
import './styles/themes.scss';
import './styles/gateway-brand.scss';
import './styles/layout.scss';
import './styles/components.scss';
import './styles/global.scss';
import { useThemeStore } from './stores/useThemeStore';

const faviconEl = document.querySelector<HTMLLinkElement>('link[rel="icon"]') ?? document.createElement('link');
faviconEl.rel = 'icon';
faviconEl.type = 'image/svg+xml';
faviconEl.href = faviconUrl;
if (!faviconEl.parentNode) {
  document.head.appendChild(faviconEl);
}

function Root() {
  const initializeTheme = useThemeStore((state) => state.initializeTheme);

  useEffect(() => initializeTheme(), [initializeTheme]);

  return <App />;
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Root />
  </StrictMode>
);
