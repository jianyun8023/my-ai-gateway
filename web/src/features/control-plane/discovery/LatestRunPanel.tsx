import type {
  DiscoveryDiff,
  LatestDiscovery
} from '@/admin-api';
import { StatusPill } from '@/components/ui/StatusPill';
import { Notice } from '@/components/ui/Notice';
import styles from '@/features/control-plane/ControlPlane.module.scss';
import { emptyDiff, statusTone } from '@/features/control-plane/discovery/model';
import { EmptyTable } from '@/features/control-plane/shared';
import { formatDateTime } from '@/utils/format';
import { useTranslation } from 'react-i18next';

function DiffColumn({
  title,
  tone,
  entries,
}: {
  title: string;
  tone: 'success' | 'warning' | 'danger';
  entries: DiscoveryDiff['added'];
}) {
  const { t } = useTranslation('console');
  return (
    <section className={styles.diffColumn}>
      <header><h3>{title}</h3><StatusPill tone={tone}>{entries.length}</StatusPill></header>
      {entries.length === 0 ? <span>{t('discovery.diff_none')}</span> : (
        <ul>{entries.map((entry) => (
          <li key={entry.upstream_model_id}>
            <code>{entry.upstream_model_id}</code>
            {entry.changed_fields.length > 0 && <small>{entry.changed_fields.join(', ')}</small>}
          </li>
        ))}</ul>
      )}
    </section>
  );
}

export function LatestRunPanel({ latest }: { latest: LatestDiscovery | null }) {
  const { t } = useTranslation('console');
  if (!latest) return <EmptyTable title={t('discovery.empty_run_title')} description={t('discovery.empty_run_desc')} />;
  const { run } = latest;
  const diff = latest.diff ?? run.diff ?? emptyDiff();
  return (
    <div className={styles.page}>
      <div className={styles.runHeader}>
        <span className={styles.primaryText}>
          <strong><StatusPill tone={statusTone(run.status)}>{run.status}</StatusPill> {t('discovery.run_badge', { id: run.id })}</strong>
          <small>{formatDateTime(run.completed_at)} · {t('discovery.run_meta', { duration: run.latency_ms, count: run.discovered_model_count })}</small>
        </span>
        <span className={styles.primaryText}>
          <strong>{run.provider_preset_id}@{run.provider_preset_version}</strong>
          <small>{t('discovery.account_http', { account: run.account_id ?? t('discovery.none'), http: run.http_status ?? t('discovery.none') })}</small>
        </span>
      </div>
      {run.status === 'unsupported' && (
        <Notice tone="warning"><strong>{t('discovery.state_unsupported')}</strong>{run.error_message && <small>{run.error_message}</small>}{run.error_code && <code>{run.error_code}</code>}</Notice>
      )}
      {run.status === 'failed' && (
        <Notice><strong>{t('discovery.state_failed')}</strong><span>{run.error_message ?? t('discovery.state_failed_desc')}</span>{run.error_code && <code>{run.error_code}</code>}</Notice>
      )}
      {run.status === 'succeeded' && run.discovered_model_count === 0 && (
        <EmptyTable title={t('discovery.state_empty')} description={t('discovery.state_empty_desc')} />
      )}
      <div className={styles.diffGrid}>
        <DiffColumn title={t('discovery.diff_column.added')} tone="success" entries={diff.added} />
        <DiffColumn title={t('discovery.diff_column.changed')} tone="warning" entries={diff.changed} />
        <DiffColumn title={t('discovery.diff_column.missing')} tone="danger" entries={diff.missing} />
      </div>
    </div>
  );
}
