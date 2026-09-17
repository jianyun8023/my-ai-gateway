import { NavLink, Paper, Text, Title } from '@mantine/core';
import { StatusPill } from '@/components/ui/StatusPill';
import { TextField } from '@/components/ui/FormField';
import { useMediaQuery } from '@mantine/hooks';
import { Modal } from '@/components/ui/Modal';
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
  IconKey,
  IconSunAsterisk,
  IconRefreshCw,
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
  authGeneration: number;
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
  focusKey?: string;
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
  focusKey = activePage,
  refreshable = false,
  children,
}: GatewayConsoleShellProps) {
  const appliedAdminKeyRef = useRef(safeSessionRead());
  const [adminKeyDraft, setAdminKeyDraft] = useState('');
  const [adminKeyConfigured, setAdminKeyConfigured] = useState(() => Boolean(safeSessionRead()));
  // Cache identity changes without exposing the session-only secret to query keys.
  const [authGeneration, setAuthGeneration] = useState(0);
  const [refreshRevision, setRefreshRevision] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const [connectionOpen, setConnectionOpen] = useState(false);
  const pageTitleRef = useRef<HTMLHeadingElement>(null);
  const previousFocusKey = useRef(focusKey);
  const mobile = useMediaQuery('(max-width: 920px)');
  const resolvedTheme = useThemeStore((state) => state.resolvedTheme);
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
    setAuthGeneration((generation) => generation + 1);
    setRefreshRevision((c) => c + 1);
  }, []);
  useEffect(() => { setMobileNavOpen(false); }, [activePage]);
  useEffect(() => { if (!mobile) setMobileNavOpen(false); }, [mobile]);
  useEffect(() => { if (mobile) setConnectionOpen(false); }, [mobile]);
  useEffect(() => {
    if (previousFocusKey.current === focusKey) return;
    previousFocusKey.current = focusKey;
    pageTitleRef.current?.focus({ preventScroll: true });
  }, [focusKey]);

  const applyAdminKey = () => {
    const k = adminKeyDraft.trim();
    appliedAdminKeyRef.current = k;
    setAdminKeyDraft('');
    setAdminKeyConfigured(Boolean(k));
    persistAdminKey(k);
    setAuthGeneration((generation) => generation + 1);
    setRefreshRevision((c) => c + 1);
    setConnectionOpen(false);
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
    authGeneration,
    clearAdminKey,
    refreshRevision,
    setRefreshing,
  }), [adminKeyConfigured, authGeneration, clearAdminKey, getAdminKey, refreshRevision]);

  const connectionControls = <div className={styles.connectionControls} role="group" aria-label={t('shell.connection_aria')}>
    <div className={styles.connectionEndpoint}>
      <Text component="span">{t('shell.endpoint_label')}</Text>
      <Text component="div" className={styles.endpointDisplay}>{gatewayEndpoint}</Text>
    </div>
    <Text component="p" className={styles.connectionStatus}>{t(adminKeyConfigured ? 'shell.key_configured' : 'shell.key_not_configured')}</Text>
    <div className={styles.keyInput}>
      <TextField className={styles.keyField} label={t('shell.admin_key_label')} aria-label={t('shell.admin_key_label')}
        autoComplete="off" spellCheck={false} type="password" required value={adminKeyDraft}
        onChange={(event) => setAdminKeyDraft(event.target.value)}
        onKeyDown={(event) => { if (event.key === 'Enter') applyAdminKey(); }}
        placeholder={t('shell.admin_key_placeholder')} />
      <Button size="sm" variant="secondary" onClick={applyAdminKey}>{t('common.apply')}</Button>
    </div>
  </div>;

  const navigation = <div id="gateway-navigation" className={styles.navigationContent}>
        <div className={styles.brand}>
          <div className={styles.brandIcon}>AG</div>
          <span className={styles.brandText}>{t('shell.brand_name')}</span>
          <span className={styles.brandVersion}>v0.3</span>
        </div>

        {/* Flat navigation with section headers */}
        <nav className={styles.sidebarNav} aria-label={t('shell.nav_aria')}>
          {navigationSections.map((section) => (
            <div key={section.label}>
              <Text component="div" className={styles.navSection}>{section.label}</Text>
              {section.pages.map((pageId) => {
                const item = navItemsById.get(pageId);
                if (!item) return null;
                const isActive = activePage === pageId;
                return (
                  <NavLink component="button" key={pageId} type="button" active={isActive}
                    aria-current={isActive ? 'page' : undefined} onClick={() => navigate(pageId)}
                    label={item.label} leftSection={item.icon}
                    rightSection={item.badge ? <StatusPill tone="danger">{item.badge}</StatusPill> : undefined} />
                );
              })}
            </div>
          ))}
        </nav>

        {mobile && <div className={styles.mobileSettings}>
          <LanguageSwitcher className={styles.mobileLanguage} />
          {connectionControls}
        </div>}

  </div>;

  return (
    <div className={styles.shell} data-od-id="console">
      {mobile ? (
        <Modal open={mobileNavOpen} variant="drawer" position="left" width={320} title={t('shell.nav_aria')} onClose={() => setMobileNavOpen(false)}>
          {navigation}
        </Modal>
      ) : <aside className={styles.sidebar} data-od-id="sidebar" aria-label={t('shell.sidebar_aria')}>{navigation}</aside>}

      <div className={styles.mainArea}>
        <Paper component="header" radius={0} className={styles.topbar} data-od-id="topbar">
          {mobile && <IconButton label={t('shell.open_nav')} aria-controls="gateway-navigation" aria-expanded={mobileNavOpen} onClick={() => setMobileNavOpen(true)}>
            <IconMenu size={20} />
          </IconButton>}
          <Text component="span" className={styles.topbarTitle}>
            {mobile ? title : navigationSections.find((section) => section.pages.includes(activePage))?.label ?? title}
          </Text>
          <div className={styles.topbarRight}>
            {!mobile && <Button variant="ghost" size="sm" aria-haspopup="dialog" aria-expanded={connectionOpen} onClick={() => setConnectionOpen(true)}>
              <IconKey size={16} />{t('shell.connection_aria')}
            </Button>}
            <div className={styles.utilityControls}>
              <IconButton label={t(resolvedTheme === 'dark' ? 'shell.switch_to_light' : 'shell.switch_to_dark')} onClick={() => setTheme(resolvedTheme === 'dark' ? 'white' : 'dark')}>
                <IconSunAsterisk size={18} />
              </IconButton>
              {!mobile && <LanguageSwitcher />}
              {refreshable && (
                <IconButton label={t('common.refresh')} onClick={() => setRefreshRevision((c) => c + 1)} loading={refreshing}>
                  <IconRefreshCw size={18} />
                </IconButton>
              )}
            </div>
          </div>
        </Paper>

        {!mobile && <Modal open={connectionOpen} title={t('shell.connection_aria')} width={480} onClose={() => setConnectionOpen(false)}>
          {connectionControls}
        </Modal>}

        <div className={styles.content}>
          <div className={styles.pageHeader}>
            <Title ref={pageTitleRef} order={1} tabIndex={-1}>{title}</Title>
          </div>
          {/* eslint-disable-next-line react-hooks/refs -- render prop pattern; ref callbacks are memoized */}
          {children(contentContext)}
        </div>
      </div>
    </div>
  );
}
