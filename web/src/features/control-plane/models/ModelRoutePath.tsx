import { Text } from '@mantine/core';
import { useTranslation } from 'react-i18next';
import { Card } from '@/components/ui/Card';
import { StatusPill, type StatusTone } from '@/components/ui/StatusPill';
import { ProtocolPill } from '@/features/control-plane/shared';
import { PROTOCOL_LABELS } from '@/lib/protocols';
import type { ModelRoutePathEntry, ModelRoutePathGroup, ModelRoutingSummary } from './routingPresentation';
import styles from './ModelRoutePath.module.scss';

const healthTone = (health: string): StatusTone => {
  if (health === 'healthy') return 'success';
  if (health === 'cooling_down' || health === 'unhealthy') return 'danger';
  if (health === 'degraded') return 'warning';
  return 'muted';
};

function RouteLine({ entry, strategy }: { entry: ModelRoutePathEntry; strategy: ModelRoutePathGroup['strategy'] }) {
  const { t } = useTranslation('console');
  const { line } = entry;
  const roleLabel = entry.role === 'primary' ? t('models.v3.primary')
    : entry.role === 'backup' ? t('models.v3.backup', { index: strategy === 'ordered' ? entry.backupIndex ?? '' : '' })
      : t('models.state.not_published');
  const healthLabel = line.healthStatus === 'disabled' ? t('models.v3.disabled')
    : t(`values.health.${line.healthStatus}`, { defaultValue: t('models.v3.unknown') });
  return (
    <Card className={styles.line}>
      <div className={styles.lineHeader}>
        <Text component="span" size="xs" fw={600} c={entry.role === 'primary' ? 'var(--accent)' : 'dimmed'} className={styles.wrap}>{roleLabel}</Text>
        <StatusPill tone={healthTone(line.healthStatus)}>{healthLabel}</StatusPill>
      </div>
      <Text size="xs" fw={600} className={styles.wrap}>{line.accountName}</Text>
      <Text component="div" size="xs" c="dimmed" className={styles.metadata}>
        <span>{line.sourceName}</span><code>{line.upstreamModelId}</code>
      </Text>
      {entry.role === 'unselected' && ['pending', 'unavailable', 'disabled'].includes(entry.status)
        && entry.status !== line.healthStatus && <StatusPill tone={entry.status === 'unavailable' ? 'danger' : 'warning'}>{t(`models.v3.${entry.status}`)}</StatusPill>}
    </Card>
  );
}

function FailureArrow() {
  const { t } = useTranslation('console');
  return <Text component="span" size="xs" c="dimmed" className={styles.arrow}
    role="img" aria-label={t('models.v3.failure_arrow')} title={t('models.v3.failure_arrow')}>
    →
  </Text>;
}

function PathGroup({ path, showProtocols }: { path: ModelRoutePathGroup; showProtocols: boolean }) {
  const { t } = useTranslation('console');
  const selected = path.entries.filter((entry) => entry.role !== 'unselected');
  const unselected = path.entries.filter((entry) => entry.role === 'unselected');
  const primary = selected.filter((entry) => entry.role === 'primary');
  const backups = selected.filter((entry) => entry.role === 'backup');
  return (
    <div className={styles.path} role="group" aria-label={path.protocols.map((protocol) => PROTOCOL_LABELS[protocol]).join(', ')}>
      {showProtocols && <div className={styles.badges}>{path.protocols.map((protocol) => <ProtocolPill key={protocol} protocol={protocol} />)}</div>}
      <div className={styles.chain}>
        {path.strategy === 'weighted' ? <>
          {primary.map((entry) => <RouteLine key={entry.line.id} entry={entry} strategy={path.strategy} />)}
          {primary.length > 0 && backups.length > 0 && <FailureArrow />}
          {backups.length > 0 && <div className={styles.pool} role="group" aria-label={t('models.v3.weighted_pool')}>
            <Text size="xs" c="dimmed">{t('models.v3.weighted_pool')}</Text>
            <div className={styles.chain}>{backups.map((entry) => <RouteLine key={entry.line.id} entry={entry} strategy={path.strategy} />)}</div>
          </div>}
        </> : selected.map((entry, index) => <div key={entry.line.id} className={styles.step}>
          {path.strategy === 'ordered' && index > 0 && entry.role === 'backup' && entry.backupIndex === index && <FailureArrow />}
          <RouteLine entry={entry} strategy={path.strategy} />
        </div>)}
        {unselected.map((entry) => <RouteLine key={entry.line.id} entry={entry} strategy={path.strategy} />)}
      </div>
    </div>
  );
}

export function ModelRoutePath({ summary }: { summary: ModelRoutingSummary }) {
  const { t } = useTranslation('console');
  if (summary.paths.length === 0) return <Text size="sm" c="dimmed">{t('models.v3.no_lines')}</Text>;
  return <div className={styles.paths}>
    {summary.protocolSpecific && <Text size="xs" c="dimmed">{t('models.v3.protocol_specific')}</Text>}
    {summary.paths.map((path) => <PathGroup key={path.protocols.join(':')} path={path} showProtocols={summary.protocolSpecific} />)}
  </div>;
}
