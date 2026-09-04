use super::*;

impl Database {
    #[allow(dead_code)]
    pub async fn insert_usage(&self, event: &UsageEvent) -> Result<(), sqlx::Error> {
        self.insert_usage_with_attempts(event, &[]).await
    }

    pub async fn insert_usage_with_attempts(
        &self,
        event: &UsageEvent,
        attempts: &[UsageAttempt],
    ) -> Result<(), sqlx::Error> {
        let now: DateTime<Utc> = Utc::now();
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO usage_events (request_id, virtual_key_id, provider_id, account_id, model, logical_model, upstream_model_id, source_id, client_source, protocol_in, protocol_upstream, mode, status_code, success, retry_count, latency_ms, ttft_ms, input_tokens, output_tokens, reasoning_tokens, cached_tokens, cache_read_tokens, cache_creation_tokens, total_tokens, usage_source, degraded, route_id, streamed, error_summary, fallback_reason, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31) ON CONFLICT (request_id) DO NOTHING")
            .bind(&event.request_id).bind(event.virtual_key_id).bind(&event.provider_id).bind(&event.account_id).bind(&event.model)
            .bind(&event.logical_model).bind(&event.upstream_model_id).bind(&event.source_id).bind(&event.client_source)
            .bind(&event.protocol_in).bind(&event.protocol_upstream).bind(&event.mode).bind(event.status_code)
            .bind(event.success).bind(event.retry_count).bind(event.latency_ms).bind(event.ttft_ms)
            .bind(event.input_tokens).bind(event.output_tokens).bind(event.reasoning_tokens)
            .bind(event.cached_tokens).bind(event.cache_read_tokens).bind(event.cache_creation_tokens)
            .bind(event.total_tokens).bind(&event.usage_source).bind(event.degraded)
            .bind(&event.route_id).bind(event.streamed).bind(&event.error_summary).bind(&event.fallback_reason).bind(now)
            .execute(&mut *tx).await?;
        for attempt in attempts {
            sqlx::query("INSERT INTO usage_event_attempts (request_id,attempt_no,provider_id,source_id,account_id,upstream_model_id,status_code,success,latency_ms) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (request_id,attempt_no) DO NOTHING")
                .bind(&event.request_id).bind(attempt.attempt_no).bind(&attempt.provider_id)
                .bind(&attempt.source_id).bind(&attempt.account_id).bind(&attempt.upstream_model_id).bind(attempt.status_code)
                .bind(attempt.success).bind(attempt.latency_ms).execute(&mut *tx).await?;
        }
        tx.commit().await
    }
    pub async fn list_usage_events_page(
        &self,
        filter: &UsageFilter,
        limit: i64,
        cursor: Option<&UsageCursor>,
    ) -> Result<UsageEventPage, sqlx::Error> {
        let limit = limit.clamp(1, 500);
        let (mut where_sql, binds) = filter_sql(filter);
        if cursor.is_some() {
            let conjunction = if where_sql.is_empty() { "WHERE" } else { "AND" };
            where_sql.push_str(&format!(
                " {conjunction} (created_at, request_id) < (${}, ${})",
                binds.len() + 1,
                binds.len() + 2
            ));
        }
        let query = format!(
            "{} {where_sql} ORDER BY created_at DESC, request_id DESC LIMIT ${}",
            usage_event_select(),
            binds.len() + if cursor.is_some() { 3 } else { 1 }
        );
        let mut q = sqlx::query_as::<_, UsageEventRecord>(&query);
        q = bind_filter(q, binds);
        if let Some(cursor) = cursor {
            q = q.bind(cursor.created_at).bind(&cursor.request_id);
        }
        let mut data = q.bind(limit + 1).fetch_all(&self.pool).await?;
        let has_more = data.len() as i64 > limit;
        if has_more {
            data.truncate(limit as usize);
        }
        let next_cursor = has_more.then(|| {
            let last = data.last().expect("a page with more rows is non-empty");
            UsageCursor {
                created_at: last.created_at,
                request_id: last.request_id.clone(),
            }
            .encode()
        });
        Ok(UsageEventPage {
            data,
            next_cursor,
            has_more,
        })
    }

    pub async fn export_usage_events(
        &self,
        filter: &UsageFilter,
        limit: i64,
    ) -> Result<Vec<UsageEventRecord>, sqlx::Error> {
        let (where_sql, binds) = filter_sql(filter);
        let query = format!(
            "{} {where_sql} ORDER BY created_at DESC, request_id DESC LIMIT ${}",
            usage_event_select(),
            binds.len() + 1
        );
        let mut q = sqlx::query_as::<_, UsageEventRecord>(&query);
        q = bind_filter(q, binds);
        q.bind(limit).fetch_all(&self.pool).await
    }

    pub async fn get_usage_event_detail(
        &self,
        request_id: &str,
    ) -> Result<Option<UsageEventRecord>, sqlx::Error> {
        sqlx::query_as::<_, UsageEventRecord>(&format!(
            "{} WHERE request_id = $1",
            usage_event_select()
        ))
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn list_attempts_for_event(
        &self,
        request_id: &str,
    ) -> Result<Vec<UsageAttemptRecord>, sqlx::Error> {
        sqlx::query_as::<_, UsageAttemptRecord>("SELECT attempt_no,provider_id,source_id,account_id,upstream_model_id,status_code,success,latency_ms,created_at FROM usage_event_attempts WHERE request_id=$1 ORDER BY attempt_no")
            .bind(request_id)
            .fetch_all(&self.pool)
            .await
    }

    #[cfg(test)]
    pub async fn delete_usage_events_for_test(&self, prefix: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM usage_events WHERE request_id LIKE $1")
            .bind(format!("{prefix}%"))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn usage_aggregate(
        &self,
        filter: &UsageFilter,
    ) -> Result<UsageAggregate, sqlx::Error> {
        let (where_sql, binds) = filter_sql(filter);
        let query = format!(
            "WITH filtered AS (SELECT * FROM usage_events {where_sql}), logical AS (SELECT COUNT(*)::BIGINT AS logical_requests, COALESCE(SUM(retry_count),0)::BIGINT AS retries, COUNT(*) FILTER (WHERE success)::BIGINT AS successes, COUNT(*) FILTER (WHERE NOT success)::BIGINT AS failures, CASE WHEN COUNT(*)=0 THEN 0 ELSE COUNT(*) FILTER (WHERE success)::DOUBLE PRECISION / COUNT(*)::DOUBLE PRECISION END AS success_rate, COALESCE(AVG(latency_ms),0)::DOUBLE PRECISION AS average_latency_ms, COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms),0)::DOUBLE PRECISION AS p95_latency_ms, COALESCE(SUM(input_tokens),0)::BIGINT AS input_tokens, COALESCE(SUM(output_tokens),0)::BIGINT AS output_tokens, COALESCE(SUM(reasoning_tokens),0)::BIGINT AS reasoning_tokens, COALESCE(SUM(cached_tokens),0)::BIGINT AS cached_tokens, COALESCE(SUM(cache_read_tokens),0)::BIGINT AS cache_read_tokens, COALESCE(SUM(cache_creation_tokens),0)::BIGINT AS cache_creation_tokens, COALESCE(SUM(total_tokens),0)::BIGINT AS total_tokens FROM filtered), attempts AS (SELECT COUNT(*)::BIGINT AS upstream_attempts FROM usage_event_attempts a JOIN filtered f ON f.request_id=a.request_id) SELECT logical.logical_requests, attempts.upstream_attempts, logical.retries, logical.successes, logical.failures, logical.success_rate, logical.average_latency_ms, logical.p95_latency_ms, logical.input_tokens, logical.output_tokens, logical.reasoning_tokens, logical.cached_tokens, logical.cache_read_tokens, logical.cache_creation_tokens, logical.total_tokens FROM logical CROSS JOIN attempts"
        );
        let mut q = sqlx::query_as::<_, UsageAggregate>(&query);
        q = bind_filter(q, binds);
        q.fetch_one(&self.pool).await
    }

    pub async fn usage_timeseries(
        &self,
        filter: &UsageFilter,
        granularity: &str,
    ) -> Result<Vec<UsageTimeBucket>, sqlx::Error> {
        let trunc = match granularity {
            "day" => "day",
            _ => "hour",
        };
        let (where_sql, binds) = filter_sql(filter);
        let bucket =
            format!("date_trunc('{trunc}', created_at AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'");
        let qualified_bucket = format!(
            "date_trunc('{trunc}', filtered.created_at AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'"
        );
        let query = format!(
            "WITH filtered AS (SELECT * FROM usage_events {where_sql}), logical AS (SELECT {bucket} AS bucket, COUNT(*)::BIGINT AS logical_requests, COALESCE(SUM(retry_count),0)::BIGINT AS retries, COUNT(*) FILTER (WHERE success)::BIGINT AS successes, COUNT(*) FILTER (WHERE NOT success)::BIGINT AS failures, COUNT(*) FILTER (WHERE success)::DOUBLE PRECISION / COUNT(*)::DOUBLE PRECISION AS success_rate, COALESCE(AVG(latency_ms),0)::DOUBLE PRECISION AS average_latency_ms, COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms),0)::DOUBLE PRECISION AS p95_latency_ms, COALESCE(SUM(input_tokens),0)::BIGINT AS input_tokens, COALESCE(SUM(output_tokens),0)::BIGINT AS output_tokens, COALESCE(SUM(reasoning_tokens),0)::BIGINT AS reasoning_tokens, COALESCE(SUM(cached_tokens),0)::BIGINT AS cached_tokens, COALESCE(SUM(cache_read_tokens),0)::BIGINT AS cache_read_tokens, COALESCE(SUM(cache_creation_tokens),0)::BIGINT AS cache_creation_tokens, COALESCE(SUM(total_tokens),0)::BIGINT AS total_tokens FROM filtered GROUP BY 1), attempts AS (SELECT {qualified_bucket} AS bucket, COUNT(*)::BIGINT AS upstream_attempts FROM filtered JOIN usage_event_attempts USING (request_id) GROUP BY 1) SELECT logical.bucket, logical.logical_requests, COALESCE(attempts.upstream_attempts,0)::BIGINT AS upstream_attempts, logical.retries, logical.successes, logical.failures, logical.success_rate, logical.average_latency_ms, logical.p95_latency_ms, logical.input_tokens, logical.output_tokens, logical.reasoning_tokens, logical.cached_tokens, logical.cache_read_tokens, logical.cache_creation_tokens, logical.total_tokens FROM logical LEFT JOIN attempts USING (bucket) ORDER BY logical.bucket"
        );
        let mut q = sqlx::query_as::<_, UsageTimeBucket>(&query);
        q = bind_filter(q, binds);
        q.fetch_all(&self.pool).await
    }

    pub async fn usage_breakdown(
        &self,
        filter: &UsageFilter,
        dimension: &str,
    ) -> Result<Vec<UsageBreakdown>, sqlx::Error> {
        let (column, qualified_column) = match dimension {
            "logical_model" => ("logical_model", "f.logical_model"),
            "upstream_model" => ("upstream_model_id", "f.upstream_model_id"),
            "provider" => ("provider_id", "f.provider_id"),
            "source_id" => ("source_id", "f.source_id"),
            "client_source" => ("client_source", "f.client_source"),
            "account" => ("account_id", "f.account_id"),
            "protocol_in" => ("protocol_in", "f.protocol_in"),
            "protocol_upstream" => ("protocol_upstream", "f.protocol_upstream"),
            "virtual_key" => ("virtual_key_id::TEXT", "f.virtual_key_id::TEXT"),
            "status" => (
                "CASE WHEN success THEN 'success' ELSE 'failure' END",
                "CASE WHEN f.success THEN 'success' ELSE 'failure' END",
            ),
            "usage_source" => ("usage_source", "f.usage_source"),
            _ => ("logical_model", "f.logical_model"),
        };
        let (where_sql, binds) = filter_sql(filter);
        let query = format!(
            "WITH filtered AS (SELECT * FROM usage_events {where_sql}), logical AS (SELECT {column} AS key, COUNT(*)::BIGINT AS logical_requests, COALESCE(SUM(retry_count),0)::BIGINT AS retries, COUNT(*) FILTER (WHERE success)::BIGINT AS successes, COUNT(*) FILTER (WHERE NOT success)::BIGINT AS failures, COUNT(*) FILTER (WHERE success)::DOUBLE PRECISION / COUNT(*)::DOUBLE PRECISION AS success_rate, COALESCE(AVG(latency_ms),0)::DOUBLE PRECISION AS average_latency_ms, COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms),0)::DOUBLE PRECISION AS p95_latency_ms, COALESCE(SUM(input_tokens),0)::BIGINT AS input_tokens, COALESCE(SUM(output_tokens),0)::BIGINT AS output_tokens, COALESCE(SUM(reasoning_tokens),0)::BIGINT AS reasoning_tokens, COALESCE(SUM(cached_tokens),0)::BIGINT AS cached_tokens, COALESCE(SUM(cache_read_tokens),0)::BIGINT AS cache_read_tokens, COALESCE(SUM(cache_creation_tokens),0)::BIGINT AS cache_creation_tokens, COALESCE(SUM(total_tokens),0)::BIGINT AS total_tokens FROM filtered GROUP BY {column}), attempts AS (SELECT {qualified_column} AS key, COUNT(*)::BIGINT AS upstream_attempts FROM filtered f JOIN usage_event_attempts a ON f.request_id=a.request_id GROUP BY {qualified_column}) SELECT logical.key, logical.logical_requests, COALESCE(attempts.upstream_attempts,0)::BIGINT AS upstream_attempts, logical.retries, logical.successes, logical.failures, logical.success_rate, logical.logical_requests::DOUBLE PRECISION / SUM(logical.logical_requests) OVER ()::DOUBLE PRECISION AS logical_request_share, CASE WHEN SUM(logical.total_tokens) OVER ()=0 THEN 0 ELSE logical.total_tokens::DOUBLE PRECISION / SUM(logical.total_tokens) OVER ()::DOUBLE PRECISION END AS total_token_share, logical.average_latency_ms, logical.p95_latency_ms, logical.input_tokens, logical.output_tokens, logical.reasoning_tokens, logical.cached_tokens, logical.cache_read_tokens, logical.cache_creation_tokens, logical.total_tokens FROM logical LEFT JOIN attempts ON logical.key IS NOT DISTINCT FROM attempts.key ORDER BY logical.logical_requests DESC, logical.key ASC NULLS LAST"
        );
        let mut q = sqlx::query_as::<_, UsageBreakdown>(&query);
        q = bind_filter(q, binds);
        q.fetch_all(&self.pool).await
    }
}

fn usage_event_select() -> &'static str {
    "SELECT request_id,virtual_key_id,provider_id,account_id,logical_model,upstream_model_id,source_id,client_source,protocol_in,protocol_upstream,mode,status_code,success,retry_count,latency_ms,ttft_ms,input_tokens,output_tokens,reasoning_tokens,cached_tokens,cache_read_tokens,cache_creation_tokens,total_tokens,usage_source,degraded,route_id,streamed,error_summary,fallback_reason,created_at FROM usage_events"
}

pub(super) fn filter_sql(filter: &UsageFilter) -> (String, Vec<FilterBind>) {
    let mut clauses = Vec::new();
    let mut binds = Vec::new();
    if let Some(value) = filter.from {
        clauses.push(format!("created_at >= ${}", binds.len() + 1));
        binds.push(FilterBind::Time(value));
    }
    if let Some(value) = filter.to {
        clauses.push(format!("created_at < ${}", binds.len() + 1));
        binds.push(FilterBind::Time(value));
    }
    for (column, value) in [
        ("logical_model", &filter.logical_model),
        ("upstream_model_id", &filter.upstream_model_id),
        ("provider_id", &filter.provider_id),
        ("source_id", &filter.source_id),
        ("client_source", &filter.client_source),
        ("account_id", &filter.account_id),
        ("protocol_in", &filter.protocol_in),
        ("protocol_upstream", &filter.protocol_upstream),
        ("usage_source", &filter.usage_source),
    ] {
        if let Some(value) = value {
            clauses.push(format!("{column} = ${}", binds.len() + 1));
            binds.push(FilterBind::Text(value.clone()));
        }
    }
    if let Some(value) = filter.virtual_key_id {
        clauses.push(format!("virtual_key_id = ${}", binds.len() + 1));
        binds.push(FilterBind::I64(value));
    }
    if let Some(value) = filter.success {
        clauses.push(format!("success = ${}", binds.len() + 1));
        binds.push(FilterBind::Bool(value));
    }
    if let Some(value) = filter.status_code {
        clauses.push(format!("status_code = ${}", binds.len() + 1));
        binds.push(FilterBind::I32(value));
    }
    let sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    (sql, binds)
}

pub(super) enum FilterBind {
    Time(DateTime<Utc>),
    Text(String),
    I64(i64),
    I32(i32),
    Bool(bool),
}
fn bind_filter<'q, O>(
    mut query: sqlx::query::QueryAs<'q, sqlx::Postgres, O, sqlx::postgres::PgArguments>,
    binds: Vec<FilterBind>,
) -> sqlx::query::QueryAs<'q, sqlx::Postgres, O, sqlx::postgres::PgArguments>
where
    O: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow>,
{
    for bind in binds {
        query = match bind {
            FilterBind::Time(value) => query.bind(value),
            FilterBind::Text(value) => query.bind(value),
            FilterBind::I64(value) => query.bind(value),
            FilterBind::I32(value) => query.bind(value),
            FilterBind::Bool(value) => query.bind(value),
        };
    }
    query
}
