import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import gatewayIcon from '@/assets/gateway-icon.svg';
import { Button } from '@/components/ui/Button';
import {
  IconChartLine,
  IconMenu,
  IconRefreshCw,
  IconSettings,
  IconX,
} from '@/components/ui/icons';
import type { GatewayConsoleSpace } from '@/lib/consoleNavigation';
import { useThemeStore } from '@/stores/useThemeStore';
import styles from './GatewayConsoleShell.module.scss';

export const GATEWAY_ADMIN_KEY_STORAGE_KEY = 'my-ai-gateway-admin-key-v1';

export interface GatewayConsoleNavItem {
  id: string;
  label: string;
  shortLabel: string;
  icon: ReactNode;
}

export interface GatewayConsoleContentContext {
  getAdminKey: () => string;
  adminKeyConfigured: boolean;
  clearAdminKey: () => void;
  refreshRevision: number;
  setRefreshing: (refreshing: boolean) => void;
}

interface GatewayConsoleShellProps {
  space: GatewayConsoleSpace;
  navigationLabel: string;
  navigationSection: string;
  navigationItems: readonly GatewayConsoleNavItem[];
  activeItem: string;
  onNavigate: (item: string) => void;
  onSpaceChange: (space: GatewayConsoleSpace) => void;
  title: string;
  shortTitle: string;
  eyebrow: string;
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
    // The key remains available in memory when browser storage is unavailable.
  }
};

export function GatewayConsoleShell({
  space,
  navigationLabel,
  navigationSection,
  navigationItems,
  activeItem,
  onNavigate,
  onSpaceChange,
  title,
  shortTitle,
  eyebrow,
  description,
  refreshable = false,
  children,
}: GatewayConsoleShellProps) {
  const appliedAdminKeyRef = useRef(safeSessionRead());
  const [adminKeyDraft, setAdminKeyDraft] = useState('');
  const [adminKeyConfigured, setAdminKeyConfigured] = useState(Boolean(appliedAdminKeyRef.current));
  const [refreshRevision, setRefreshRevision] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const menuButtonRef = useRef<HTMLButtonElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const theme = useThemeStore((state) => state.theme);
  const setTheme = useThemeStore((state) => state.setTheme);
  const localTimeZone = useMemo(() => Intl.DateTimeFormat().resolvedOptions().timeZone, []);

  const getAdminKey = useCallback(() => appliedAdminKeyRef.current, []);
  const clearAdminKey = useCallback(() => {
    appliedAdminKeyRef.current = '';
    setAdminKeyDraft('');
    setAdminKeyConfigured(false);
    persistAdminKey('');
    setRefreshRevision((current) => current + 1);
  }, []);
  const closeMobileNav = useCallback((restoreFocus = false) => {
    setMobileNavOpen(false);
    if (restoreFocus) window.setTimeout(() => menuButtonRef.current?.focus(), 0);
  }, []);

  useEffect(() => {
    setMobileNavOpen(false);
  }, [activeItem, space]);

  useEffect(() => {
    if (!mobileNavOpen) return;
    const previousOverflow = document.body.style.overflow;
    const focusTimer = window.setTimeout(() => closeButtonRef.current?.focus(), 0);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closeMobileNav(true);
    };
    document.body.style.overflow = 'hidden';
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.clearTimeout(focusTimer);
      window.removeEventListener('keydown', onKeyDown);
      document.body.style.overflow = previousOverflow;
    };
  }, [closeMobileNav, mobileNavOpen]);

  useEffect(() => {
    const media = window.matchMedia?.('(max-width: 920px)');
    if (!media) return;
    const closeOnDesktop = () => {
      if (!media.matches) setMobileNavOpen(false);
    };
    media.addEventListener('change', closeOnDesktop);
    return () => media.removeEventListener('change', closeOnDesktop);
  }, []);

  const applyAdminKey = () => {
    const nextAdminKey = adminKeyDraft.trim();
    appliedAdminKeyRef.current = nextAdminKey;
    setAdminKeyDraft('');
    setAdminKeyConfigured(Boolean(nextAdminKey));
    persistAdminKey(nextAdminKey);
    setRefreshRevision((current) => current + 1);
  };

  const navigate = (item: string) => {
    setMobileNavOpen(false);
    onNavigate(item);
  };

  const changeSpace = (nextSpace: GatewayConsoleSpace) => {
    setMobileNavOpen(false);
    onSpaceChange(nextSpace);
  };

  const contentContext = useMemo<GatewayConsoleContentContext>(() => ({
    getAdminKey,
    adminKeyConfigured,
    clearAdminKey,
    refreshRevision,
    setRefreshing,
  }), [adminKeyConfigured, clearAdminKey, getAdminKey, refreshRevision]);

  return (
    <div className={styles.shell} data-space={space} data-od-id={`console-${space}`}>
      <aside id="gateway-navigation" className={styles.sidebar} data-open={mobileNavOpen} data-od-id="sidebar" aria-label="控制台侧栏">
        <div className={styles.brand}>
          <img src={gatewayIcon} alt="" />
          <div><strong>AI Gateway</strong><small>my-ai-gateway</small></div>
          <button ref={closeButtonRef} type="button" className={styles.sidebarClose} aria-label="关闭导航" onClick={() => closeMobileNav(true)}><IconX size={18} /></button>
        </div>

        <div className={styles.spaceSwitch} role="group" aria-label="工作空间">
          <button type="button" data-active={space === 'usage'} onClick={() => changeSpace('usage')}><IconChartLine size={16} /><span>Usage</span></button>
          <button type="button" data-active={space === 'management'} onClick={() => changeSpace('management')}><IconSettings size={16} /><span>Management</span></button>
        </div>

        <nav aria-label={navigationLabel}>
          <span className={styles.navSection}>{navigationSection}</span>
          {navigationItems.map((item) => (
            <button key={item.id} type="button" data-active={activeItem === item.id} aria-current={activeItem === item.id ? 'page' : undefined} onClick={() => navigate(item.id)}>
              <span className={styles.navIcon}>{item.icon}</span>
              <span><strong>{item.label}</strong><small>{item.shortLabel}</small></span>
            </button>
          ))}
        </nav>
        <div className={styles.sidebarFooter}><strong>UTC</strong><span>PostgreSQL 存储边界</span><small>{localTimeZone} 展示</small></div>
      </aside>
      <button type="button" className={styles.mobileOverlay} data-open={mobileNavOpen} aria-label="关闭导航遮罩" tabIndex={mobileNavOpen ? 0 : -1} onClick={() => closeMobileNav(true)} />

      <section className={styles.workspace}>
        <header className={styles.topbar} data-od-id="topbar">
          <button ref={menuButtonRef} type="button" className={styles.mobileMenuButton} aria-label="打开导航" aria-controls="gateway-navigation" aria-expanded={mobileNavOpen} onClick={() => setMobileNavOpen(true)}><IconMenu size={20} /></button>
          <div className={styles.topbarTitle}><strong>{title}</strong><small>{shortTitle}</small></div>
          <div className={styles.headerActions}>
            <label className={styles.keyInput}><span>Admin Key</span><input aria-label="Admin Key" autoComplete="off" spellCheck={false} type="password" required value={adminKeyDraft} onChange={(event) => setAdminKeyDraft(event.target.value)} placeholder="输入 GATEWAY_ADMIN_KEY" /><Button size="sm" variant="secondary" onClick={applyAdminKey}>应用</Button></label>
            <Button size="sm" variant="ghost" onClick={() => setTheme(theme === 'dark' ? 'white' : 'dark')}>{theme === 'dark' ? '浅色' : '深色'}</Button>
            {refreshable && <Button size="sm" variant="secondary" onClick={() => setRefreshRevision((current) => current + 1)} loading={refreshing}><IconRefreshCw size={14} />刷新</Button>}
          </div>
        </header>

        <main className={styles.main}>
          <section className={styles.pageHeading}>
            <div><span>{eyebrow}</span><h1>{title}</h1><p>{description} · 本地时区：{localTimeZone}</p></div>
          </section>
          {children(contentContext)}
        </main>
        <footer className={styles.footer}>my-ai-gateway · UI interactions adapted from CPA Usage Keeper under the MIT License</footer>
      </section>
    </div>
  );
}
