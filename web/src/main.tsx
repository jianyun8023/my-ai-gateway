// Base/reset first, Mantine second, then component modules loaded by Root.
import './styles/global.scss';
import './styles/gateway-brand.scss';
import './styles/mantine.css';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import './i18n/console';
import { Root } from './Root';
import faviconUrl from './assets/gateway-icon.svg';

const faviconEl = document.querySelector<HTMLLinkElement>('link[rel="icon"]') ?? document.createElement('link');
faviconEl.rel = 'icon';
faviconEl.type = 'image/svg+xml';
faviconEl.href = faviconUrl;
if (!faviconEl.parentNode) {
  document.head.appendChild(faviconEl);
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Root />
  </StrictMode>
);
