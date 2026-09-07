import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { IconButton } from '@/components/ui/IconButton';
import { Button } from '@/components/ui/Button';
import {
  IconMenu,
  IconSunAsterisk,
  IconRefreshCw,
  IconX,
} from '@/components/ui/icons';
import { LanguageSwitcher } from '@/components/ui/LanguageSwitcher';
import type { ConsolePage, ConsoleNavSection } from '@/lib/consoleNavigation';
import { useThemeStore } from '@/stores/useThemeStore';
import { useTranslation } from 'react-i18next';
import styles from './GatewayConsoleShell.module.scss';

export const GATEWAY_ADMIN_KEY_STORAGE_KEY = 'my-ai-gateway-admin-key-v1';

export interface GatewayConsoleNavItem {
  id: string;
  label: string;
  shortLabel?: string;
  icon: ReactNode;
  badge?: string;
}

export interface GatewayConsoleContentContext {
  getAdminKey: () => string;
  adminKeyConfigured: boolean;
  clearAdminKey: () => void;
  refreshRevision: number;
  setRefreshing: (refreshing: boolean) => void;
}

interface GatewayConsoleShellProps {
  activePage: ConsolePage;
  navigationSections: readonly ConsoleNavSection[];
  navigationItems: readonly GatewayConsoleNavItem[];
  onNavigate: (page: string) => void;
  title: string;
  description: string;
  refreshable?: boolean;
  children: (context: GatewayConsoleContentContext) => ReactNode;
}

const safeSessionRead = (): string => {
  try {
    return sessionStorage.getItem(GATEWAY_ADMIN_KEY_STORAGE_KEY) ?? '';
  } catch {
    return '';
  }
};

const persistAdminKey = (value: string) => {
  try {
    if (value) sessionStorage.setItem(GATEWAY_ADMIN_KEY_STORAGE_KEY, value);
    else sessionStorage.removeItem(GATEWAY_ADMIN_KEY_STORAGE_KEY);
  } catch {
    // in-memory fallback
  }
};

export function GatewayConsoleShell({
  activePage,
  navigationSections,
  navigationItems,
  onNavigate,
  title,
  description,
  refreshable = false,
  children,
}: GatewayConsoleShellProps) {
  const appliedAdminKeyRef = useRef(safeSessionRead());
  const [adminKeyDraft, setAdminKeyDraft] = useState('');
  const [adminKeyConfigured, setAdminKeyConfigured] = useState(() => Boolean(safeSessionRead()));
  const [refreshRevision, setRefreshRevision] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const menuButtonRef = useRef<HTMLButtonElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const theme = useThemeStore((state) => state.theme);
  const setTheme = useThemeStore((state) => state.setTheme);
  const { t } = useTranslation('console');

  // 网关服务入口:开发态走本机后端,生产态展示部署来源(替代写死的 localhost 占位)。
  const gatewayEndpoint = import.meta.env.DEV
    ? (import.meta.env.VITE_API_PROXY_TARGET as string | undefined)?.trim() || 'http://127.0.0.1:8787'
    : window.location.origin;

  const getAdminKey = useCallback(() => appliedAdminKeyRef.current, []);
  const clearAdminKey = useCallback(() => {
    appliedAdminKeyRef.current = '';
    setAdminKeyDraft('');
    setAdminKeyConfigured(false);
    persistAdminKey('');
    setRefreshRevision((c) => c + 1);
  }, []);
  const closeMobileNav = useCallback((restoreFocus = false) => {
    setMobileNavOpen(false);
    if (restoreFocus) window.setTimeout(() => menuButtonRef.current?.focus(), 0);
  }, []);

  useEffect(() => { setMobileNavOpen(false); }, [activePage]);

  useEffect(() => {
    if (!mobileNavOpen) return;
    const prev = document.body.style.overflow;
    const t = window.setTimeout(() => closeButtonRef.current?.focus(), 0);
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') closeMobileNav(true); };
    document.body.style.overflow = 'hidden';
    window.addEventListener('keydown', onKey);
    return () => { window.clearTimeout(t); window.removeEventListener('keydown', onKey); document.body.style.overflow = prev; };
  }, [closeMobileNav, mobileNavOpen]);

  useEffect(() => {
    const mq = window.matchMedia?.('(max-width: 920px)');
    if (!mq) return;
    const close = () => { if (!mq.matches) setMobileNavOpen(false); };
    mq.addEventListener('change', close);
    return () => mq.removeEventListener('change', close);
  }, []);

  const applyAdminKey = () => {
    const k = adminKeyDraft.trim();
    appliedAdminKeyRef.current = k;
    setAdminKeyDraft('');
    setAdminKeyConfigured(Boolean(k));
    persistAdminKey(k);
    setRefreshRevision((c) => c + 1);
  };

  const navigate = (id: string) => {
    setMobileNavOpen(false);
    onNavigate(id);
  };

  const navItemsById = useMemo(() => {
    const map = new Map<string, GatewayConsoleNavItem>();
    for (const item of navigationItems) map.set(item.id, item);
    return map;
  }, [navigationItems]);

  const contentContext = useMemo<GatewayConsoleContentContext>(() => ({
    getAdminKey,
    adminKeyConfigured,
    clearAdminKey,
    refreshRevision,
    setRefreshing,
  }), [adminKeyConfigured, clearAdminKey, getAdminKey, refreshRevision]);

  return (
    <div className={styles.shell} data-od-id="console">
      <aside id="gateway-navigation" className={styles.sidebar} data-open={mobileNavOpen} data-od-id="sidebar" aria-label={t('shell.sidebar_aria')}>
        <button ref={closeButtonRef} type="button" className={styles.sidebarClose} aria-label={t('shell.close_nav')} onClick={() => closeMobileNav(true)}>
          <IconX size={18} />
        </button>

        {/* Brand — matches prototype: AG icon + AI Gateway + version */}
        <div className={styles.brand}>
          <div className={styles.brandIcon}>AG</div>
          <span className={styles.brandText}>{t('shell.brand_name')}</span>
          <span className={styles.brandVersion}>v0.3</span>
        </div>

        {/* Flat navigation with section headers */}
        <nav className={styles.sidebarNav} aria-label={t('shell.nav_aria')}>
          {navigationSections.map((section) => (
            <div key={section.label}>
              <span className={styles.navSection}>{section.label}</span>
              {section.pages.map((pageId) => {
                const item = navItemsById.get(pageId);
                if (!item) return null;
                const isActive = activePage === pageId;
                return (
                  <button
                    key={pageId}
                    type="button"
                    className={`${styles.navItem} ${isActive ? styles.navItemActive : ''}`}
                    aria-current={isActive ? 'page' : undefined}
                    onClick={() => navigate(pageId)}
                  >
                    {item.icon}
                    <span>{item.label}</span>
                    {item.badge && <span className={styles.navBadge}>{item.badge}</span>}
                  </button>
                );
              })}
            </div>
          ))}
        </nav>

        {/* Drawer language switch (≤920px topbar copy hidden) — visible inside the mobile sidebar */}
        <div className={styles.sidebarLanguageArea}>
          <LanguageSwitcher />
        </div>

        {/* Mobile-only admin key — visible in sidebar when topbar input is hidden */}
        <div className={styles.mobileKeySection}>
          <label className={styles.mobileKeyLabel}>
            <span>{t('shell.mobile_key_label')}</span>
            <input
              autoComplete="off"
              spellCheck={false}
              type="password"
              value={adminKeyDraft}
              onChange={(e) => setAdminKeyDraft(e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Enter') applyAdminKey(); }}
              placeholder={t('shell.admin_key_placeholder')}
            />
          </label>
          <Button size="sm" variant="secondary" onClick={applyAdminKey}>{t('shell.mobile_key_apply')}</Button>
        </div>

        {/* Footer — matches prototype: status dot + running info */}
        <div className={styles.sidebarFooter}>
          <div className={styles.statusDot} />
          <span>{t('shell.running_status')}</span>
        </div>
      </aside>

      <button type="button" className={styles.mobileOverlay} data-open={mobileNavOpen} aria-label={t('shell.close_overlay')} tabIndex={mobileNavOpen ? 0 : -1} onClick={() => closeMobileNav(true)} />

      <div className={styles.mainArea}>
        {/* Topbar — matches prototype: title + endpoint + search + admin key */}
        <header className={styles.topbar} data-od-id="topbar">
          <button ref={menuButtonRef} type="button" className={styles.mobileMenuBtn} aria-label={t('shell.open_nav')} aria-controls="gateway-navigation" aria-expanded={mobileNavOpen} onClick={() => setMobileNavOpen(true)}>
            <IconMenu size={20} />
          </button>
          <span className={styles.topbarTitle}>{title}</span>
          <div className={styles.topbarRight}>
            <div className={styles.endpointDisplay}>
              <div className={styles.endpointDot} />
              <span>{gatewayEndpoint}</span>
            </div>
            <div className={styles.topbarSearch}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="11" cy="11" r="8" /><path d="m21 21-4.3-4.3" /></svg>
              {t('shell.search')}
              <kbd>{t('shell.search_hint')}</kbd>
            </div>
            <label className={styles.keyInput}>
              <span>{t('shell.admin_key_label')}</span>
              <input
                aria-label={t('shell.admin_key_label')}
                autoComplete="off"
                spellCheck={false}
                type="password"
                required
                value={adminKeyDraft}
                onChange={(e) => setAdminKeyDraft(e.target.value)}
                onKeyDown={(e) => { if (e.key === 'Enter') applyAdminKey(); }}
                placeholder={t('shell.admin_key_placeholder')}
              />
              <Button size="sm" variant="secondary" onClick={applyAdminKey}>{t('common.apply')}</Button>
            </label>
            <IconButton label={t(theme === 'dark' ? 'shell.switch_to_light' : 'shell.switch_to_dark')} onClick={() => setTheme(theme === 'dark' ? 'white' : 'dark')}>
              <IconSunAsterisk size={18} />
            </IconButton>
            <div className={styles.topbarLanguage}>
              <LanguageSwitcher />
            </div>
            {refreshable && (
              <Button size="sm" variant="secondary" aria-label={t('common.refresh')} title={t('common.refresh')} onClick={() => setRefreshRevision((c) => c + 1)} loading={refreshing}>
                <IconRefreshCw size={14} />
              </Button>
            )}
          </div>
        </header>

        {/* Content — matches prototype: simple h1 + description + content */}
        <div className={styles.content}>
          <div className={styles.pageHeader}>
            <div>
              <h1>{title}</h1>
              <p className={styles.pageDesc}>{description}</p>
            </div>
          </div>
          {/* eslint-disable-next-line react-hooks/refs -- render prop pattern; ref callbacks are memoized */}
          {children(contentContext)}
        </div>
      </div>
    </div>
  );
}
