use super::*;

#[derive(Clone, Copy, Debug)]
pub(crate) enum UsageOptionField {
    LogicalModel,
    UpstreamModel,
    Provider,
    Source,
    Account,
    ClientSource,
    VirtualKey,
}

impl UsageOptionField {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "logical_model" => Self::LogicalModel,
            "upstream_model" => Self::UpstreamModel,
            "provider" => Self::Provider,
            "source_id" => Self::Source,
            "account" => Self::Account,
            "client_source" => Self::ClientSource,
            "virtual_key" => Self::VirtualKey,
            _ => return None,
        })
    }

    fn column(self) -> &'static str {
        match self {
            Self::LogicalModel => "logical_model",
            Self::UpstreamModel => "upstream_model_id",
            Self::Provider => "provider_id",
            Self::Source => "source_id",
            Self::Account => "account_id",
            Self::ClientSource => "client_source",
            Self::VirtualKey => "virtual_key_id::text",
        }
    }
}

impl Database {
    pub(crate) async fn usage_filter_options(
        &self,
        field: UsageOptionField,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
        search: &str,
        limit: i64,
    ) -> Result<Vec<String>, sqlx::Error> {
        // Only the closed enum supplies SQL identifiers. Search is a literal
        // substring, so '%' and '_' never become LIKE wildcards.
        let column = field.column();
        let sql = format!(
            "SELECT DISTINCT {column} COLLATE \"C\" AS value FROM usage_events
             WHERE ($1::timestamptz IS NULL OR created_at >= $1)
               AND ($2::timestamptz IS NULL OR created_at < $2)
               AND {column} IS NOT NULL AND btrim({column}) <> ''
               AND strpos(lower({column}), lower($3)) > 0
             ORDER BY value LIMIT $4"
        );
        let result = sqlx::query_scalar(&sql)
            .bind(from)
            .bind(to)
            .bind(search)
            .bind(limit.clamp(1, 100) + 1)
            .fetch_all(&self.pool)
            .await;
        self.events.observe("usage.filter_options", result).await
    }
}
