import { Modal } from '@/components/ui/Modal';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { Notice } from '@/components/ui/Notice';
import styles from '@/features/usage/Usage.module.scss';
import { UsageBadge } from '@/features/usage/UsageBadge';
import { formatFallbackReason, formatTime } from '@/features/usage/formatters';
import { GatewayUsageClient, type UsageEventViewModel } from '@/gateway-usage';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
import { formatExactInteger } from '@/utils/formatCompact';
import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';

export function EventDetails({ event, onClose, client }: { event: UsageEventViewModel; onClose: () => void; client: GatewayUsageClient }) {
  const { t } = useTranslation('console');
  const localizeError = useLocalizedApiError();
  const load = useCallback((signal: AbortSignal) => client.eventDetail(event.requestId, signal), [client, event.requestId]);
  const query = useAdminQuery({ load });
  const loadingAttempts = query.loading || query.refreshing;
  const displayAttempts = query.data ?? event.attempts;

  return (
    <Modal open variant="drawer" width={520} title={event.requestId} onClose={onClose} footer={<Button variant="secondary" onClick={onClose}>{t('common.close')}</Button>}>
      <div className={styles.stack} data-od-id="event-drawer">
        <section className={styles.detailGrid}>
          <div><span>{t('usage.field.time')}</span><strong>{formatTime(event.createdAt)}</strong></div>
          <div><span>{t('usage.field.status')}</span><strong>{event.statusCode} · {event.success ? t('usage.event.success') : t('usage.event.failure')}</strong></div>
          <div><span>{t('usage.field.logical_model')}</span><strong>{event.logicalModel}</strong></div>
          <div><span>{t('usage.field.upstream_model')}</span><strong>{event.upstreamModel}</strong></div>
          <div><span>{t('usage.field.provider')}</span><strong>{event.provider}</strong></div>
          <div><span>{t('usage.field.source_id')}</span><strong>{event.sourceId}</strong></div>
          <div><span>{t('usage.field.client_source')}</span><strong>{event.clientSource}</strong></div>
          <div><span>{t('usage.field.account')}</span><strong>{event.account}</strong></div>
          <div><span>{t('usage.field.protocol')}</span><strong>{event.protocolIn} → {event.protocolUpstream}</strong></div>
          <div><span>{t('usage.field.usage_source')}</span><strong><UsageBadge source={event.usageSource} /></strong></div>
          <div><span>{t('usage.field.latency')}</span><strong>{formatExactInteger(event.latencyMs)} ms</strong></div>
          <div><span>{t('usage.field.retries')}</span><strong>{event.fallback ? (event.retryCount > 0 ? t('usage.event.retries_fallback', { count: event.retryCount }) : t('usage.event.fallback_only')) : String(event.retryCount)}</strong></div>
          {event.fallbackReason && (
            <div><span>{t('usage.field.fallback_reason')}</span><strong title={event.fallbackReason}>{formatFallbackReason(t, event.fallbackReason)}</strong></div>
          )}
        </section>
        <Card title={t('usage.detail.token_title')} subtitle={t('usage.detail.token_subtitle')}>
          <div className={styles.tokenDetails}><span>{t('usage.legend.input')} <strong>{event.tokens.input}</strong></span><span>{t('usage.legend.output')} <strong>{event.tokens.output}</strong></span><span>{t('usage.legend.reasoning')} <strong>{event.tokens.reasoning}</strong></span><span>{t('usage.legend.cache_read')} <strong>{event.tokens.cacheRead}</strong></span><span>{t('usage.legend.cache_creation')} <strong>{event.tokens.cacheCreation}</strong></span><span>{t('usage.legend.total')} <strong>{event.tokens.total}</strong></span></div>
        </Card>
        {event.fallback && event.fallbackReason && event.retryCount === 0 && (
          <p className={styles.fallbackNotice}>{t('usage.detail.primary_skipped')}</p>
        )}
        <Card title={t('usage.detail.attempts_title')} subtitle={t('usage.detail.attempts_subtitle')}>
          {query.error && <Notice action={<Button size="sm" variant="secondary" onClick={query.reload}>{t('common.retry')}</Button>}>{localizeError(query.error)}</Notice>}
          {loadingAttempts ? <div style={{ padding: '1rem', opacity: 0.6 }}>{t('usage.detail.attempts_loading')}</div> : query.error && displayAttempts.length === 0 ? null : displayAttempts.length === 0 ? <EmptyState title={t('usage.detail.attempts_empty_title')} description={t('usage.detail.attempts_empty_desc')} /> : (
            <ol className={styles.attemptList}>{displayAttempts.map((attempt) => <li key={attempt.attemptIndex}><span>#{attempt.attemptIndex + 1}</span><strong>{attempt.account}</strong><span>{attempt.sourceId} · {attempt.provider} · {attempt.upstreamModel}</span><span className={attempt.success ? styles.statusSuccess : styles.statusFailure}>{attempt.statusCode} · {attempt.latencyMs} ms</span></li>)}</ol>
          )}
        </Card>
        {event.errorSummary && <Card title={t('usage.detail.error_summary')}><p className={styles.errorSummary}>{event.errorSummary}</p></Card>}
      </div>
    </Modal>
  );
}
