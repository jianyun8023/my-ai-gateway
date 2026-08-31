import { EmptyState } from '@/components/ui/EmptyState';
import type { GatewayManagementPage } from '@/lib/consoleNavigation';
import styles from './GatewayManagementPage.module.scss';

const PAGE_LABELS: Record<GatewayManagementPage, string> = {
  sources: 'Sources',
  'model-discovery': 'Model Discovery',
  capabilities: 'Effective Capabilities',
};

export function GatewayManagementPage({ page }: { page: GatewayManagementPage }) {
  return (
    <section className={styles.empty} aria-label={`${PAGE_LABELS[page]} 管理页`}>
      <EmptyState
        title="功能尚未接入"
        description={`${PAGE_LABELS[page]} 将在对应管理任务中接入真实 Admin API。`}
      />
    </section>
  );
}
