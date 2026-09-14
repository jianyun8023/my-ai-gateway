import { DetailItem, DetailList } from '@/components/ui/DetailList';
import { LoadingState } from '@/components/ui/LoadingState';
import { Modal } from '@/components/ui/Modal';
import { Button } from '@/components/ui/Button';
import { EmptyState } from '@/components/ui/EmptyState';
import { Notice } from '@/components/ui/Notice';
import styles from '@/features/usage/Usage.module.scss';
import { UsageStatus } from './UsageStatus';
import { cacheHitRate, isUnreportedUsage } from './usageQuality';
import { UsageBadge } from '@/features/usage/UsageBadge';
import { formatDuration, formatFallbackReason, formatTime, formatUsageTokens } from '@/features/usage/formatters';
import { GatewayUsageClient, type UsageAttemptViewModel, type UsageEventViewModel } from '@/gateway-usage';
import { useAdminQuery } from '@/hooks/useAdminQuery';
import { useLocalizedApiError } from '@/hooks/useLocalizedApiError';
import { formatPercent } from '@/utils/formatCompact';
import { useCallback, useState, type PropsWithChildren } from 'react';
import { useTranslation } from 'react-i18next';

function DrawerSection({ title, hint, children }: PropsWithChildren<{ title: string; hint?: string }>) {
  return (
    <section className={styles.drawerSection}>
      <h3 className={styles.drawerSectionTitle}>{title}</h3>
      {hint && <p className={styles.drawerSectionHint}>{hint}</p>}
      {children}
    </section>
  );
}

// A normal request has exactly one upstream attempt: compress it to a single
// line; only retries/fallbacks expand into the per-attempt list.
function AttemptRow({ attempt }: { attempt: UsageAttemptViewModel }) {
  return (
    <div className={styles.attemptCompact} title={`${attempt.sourceId} · ${attempt.provider} · ${attempt.upstreamModel}`}>
      <span>#{attempt.attemptIndex + 1}</span>
      <strong>{attempt.account}</strong>
      <span>{attempt.protocolUpstream}</span>
      <UsageStatus success={attempt.success} statusCode={attempt.statusCode} />
      <span>{formatDuration(attempt.latencyMs, true)}</span>
    </div>
  );
}

export function EventDetails({ event, onClose, client }: { event: UsageEventViewModel; onClose: () => void; client: GatewayUsageClient }) {
  const { t } = useTranslation('console');
  const [open, setOpen] = useState(true);
  const localizeError = useLocalizedApiError();
  const load = useCallback((signal: AbortSignal) => client.eventDetail(event.requestId, signal), [client, event.requestId]);
  const query = useAdminQuery({ load });
  const loadingAttempts = query.loading || query.refreshing;
  const displayAttempts = query.data ?? event.attempts;
  const hitRate = cacheHitRate(event.tokens);
  const retryText = event.fallback
    ? (event.retryCount > 0 ? t('usage.event.retries_fallback', { count: event.retryCount }) : t('usage.event.fallback_only'))
    : String(event.retryCount);

  return (
    <Modal open={open} variant="drawer" width={520} title={event.requestId} onClose={() => setOpen(false)} onExitTransitionEnd={onClose} footer={<Button variant="secondary" onClick={() => setOpen(false)}>{t('common.close')}</Button>}>
      <div className={styles.stack} data-od-id="event-drawer">
        <div className={styles.drawerMeta}>
          <UsageStatus success={event.success} statusCode={event.statusCode} />
          <span>{formatDuration(event.latencyMs, true)} · {t('usage.field.retries')} {retryText}</span>
        </div>
        {event.fallback && event.fallbackReason && event.retryCount === 0 && (
          <p className={styles.fallbackNotice}>{t('usage.detail.primary_skipped')}</p>
        )}
        <DrawerSection title={t('usage.detail.section_basic')}>
          <DetailList>
            <DetailItem label={t('usage.field.time')}><strong>{formatTime(event.createdAt)}</strong></DetailItem>
            <DetailItem label={t('usage.field.client_source')}><strong>{event.clientSource}</strong></DetailItem>
            <DetailItem label={t('usage.field.account')}><strong>{event.account}</strong></DetailItem>
            <DetailItem label={t('usage.field.protocol')}><strong>{event.protocolIn} → {event.protocolUpstream}</strong></DetailItem>
            <DetailItem label={t('usage.field.usage_source')}><UsageBadge source={event.usageSource} /></DetailItem>
            {event.fallbackReason && (
              <DetailItem label={t('usage.field.fallback_reason')}><strong title={event.fallbackReason}>{formatFallbackReason(t, event.fallbackReason)}</strong></DetailItem>
            )}
          </DetailList>
        </DrawerSection>
        <DrawerSection title={t('usage.detail.section_routing')}>
          <DetailList>
            <DetailItem label={t('usage.field.logical_model')}><strong>{event.logicalModel}</strong></DetailItem>
            <DetailItem label={t('usage.field.upstream_model')}><strong>{event.upstreamModel}</strong></DetailItem>
            <DetailItem label={t('usage.field.provider')}><strong>{event.provider}</strong></DetailItem>
            <DetailItem label={t('usage.field.source_id')}><strong>{event.sourceId}</strong></DetailItem>
          </DetailList>
        </DrawerSection>
        <DrawerSection title={t('usage.detail.token_title')} hint={t('usage.detail.token_subtitle')}>
          {isUnreportedUsage(event.usageSource) && <p className={styles.cacheNote}>{t(event.usageSource === 'missing' ? 'usage.composition.unreported' : 'usage.composition.unknown', { count: 1 })}</p>}
          <DetailList>
            <DetailItem label={t('usage.legend.input')}><strong>{formatUsageTokens(event.tokens.input, event.usageSource, true)}</strong></DetailItem>
            <DetailItem label={t('usage.legend.output')}><strong>{formatUsageTokens(event.tokens.output, event.usageSource, true)}</strong></DetailItem>
            <DetailItem label={t('usage.legend.reasoning')}><strong>{formatUsageTokens(event.tokens.reasoning, event.usageSource, true)}</strong></DetailItem>
            <DetailItem label={t('usage.legend.total')}><strong>{formatUsageTokens(event.tokens.total, event.usageSource, true)}</strong></DetailItem>
          </DetailList>
        </DrawerSection>
        <DrawerSection title={t('usage.detail.section_cache')} hint={t('usage.events.cache_basis')}>
          <DetailList>
            <DetailItem label={t('usage.legend.cache_read')}><strong>{formatUsageTokens(event.tokens.cacheRead, event.usageSource, true)}</strong></DetailItem>
            <DetailItem label={t('usage.legend.cache_creation')}><strong>{formatUsageTokens(event.tokens.cacheCreation, event.usageSource, true)}</strong></DetailItem>
            <DetailItem label={t('usage.field.cache_hit_rate')}><strong>{hitRate === null ? '—' : formatPercent(hitRate)}</strong></DetailItem>
          </DetailList>
        </DrawerSection>
        <DrawerSection title={t('usage.detail.attempts_title')} hint={t('usage.detail.attempts_subtitle')}>
          {query.error && <Notice action={<Button size="sm" variant="secondary" onClick={query.reload}>{t('common.retry')}</Button>}>{localizeError(query.error)}</Notice>}
          {loadingAttempts ? <LoadingState layout="inline" label={t('usage.detail.attempts_loading')} /> : query.error && displayAttempts.length === 0 ? null : displayAttempts.length === 0 ? <EmptyState title={t('usage.detail.attempts_empty_title')} description={t('usage.detail.attempts_empty_desc')} /> : displayAttempts.length === 1 ? <AttemptRow attempt={displayAttempts[0]} /> : (
            <ol className={styles.attemptList}>{displayAttempts.map((attempt) => <li key={attempt.attemptIndex}><span>#{attempt.attemptIndex + 1}</span><strong>{attempt.account}</strong><span>{attempt.sourceId} · {attempt.provider} · {attempt.upstreamModel}</span><div><UsageStatus success={attempt.success} statusCode={attempt.statusCode} /><span>{formatDuration(attempt.latencyMs, true)}</span><span>{attempt.protocolUpstream}</span></div></li>)}
          </ol>
          )}
        </DrawerSection>
        {event.errorSummary && <DrawerSection title={t('usage.detail.error_summary')}><p className={styles.errorSummary}>{event.errorSummary}</p></DrawerSection>}
      </div>
    </Modal>
  );
}
