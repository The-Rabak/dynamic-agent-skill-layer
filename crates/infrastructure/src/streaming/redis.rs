use chrono::{DateTime, Utc};
use redis::{AsyncCommands, streams::StreamReadOptions};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventEnvelope {
    pub event_id: Uuid,
    pub event_type: String,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    pub schema_version: u32,
    pub timestamp: DateTime<Utc>,
    pub payload: Value,
}

impl EventEnvelope {
    pub fn new(
        event_type: impl Into<String>,
        idempotency_key: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            event_type: event_type.into(),
            correlation_id: Uuid::now_v7(),
            idempotency_key: idempotency_key.into(),
            schema_version: 1,
            timestamp: Utc::now(),
            payload,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RedisStreamsConfig {
    pub redis_url: String,
    pub stream_key: String,
    pub consumer_group: String,
    pub consumer_name: String,
    pub idempotency_ttl_secs: u64,
    pub max_stream_len: usize,
}

impl Default for RedisStreamsConfig {
    fn default() -> Self {
        Self {
            redis_url: "redis://127.0.0.1/".to_owned(),
            stream_key: "skill-layer-events".to_owned(),
            consumer_group: "skill-layer".to_owned(),
            consumer_name: "worker-1".to_owned(),
            idempotency_ttl_secs: 86_400,
            max_stream_len: 100_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamMessage {
    pub stream_id: String,
    pub envelope: EventEnvelope,
}

#[derive(Debug, Error)]
pub enum RedisStreamError {
    #[error("invalid redis streams configuration: {0}")]
    InvalidConfiguration(String),
    #[error("consumer group initialization failed (code: {code}): {detail}")]
    ConsumerGroupInitialization { code: String, detail: String },
    #[error("xack must acknowledge exactly one message for `{stream_id}`, got {actual}")]
    UnexpectedAckCount { stream_id: String, actual: i64 },
    #[error("redis streams operation failed: {0}")]
    Redis(#[from] redis::RedisError),
    #[error("event envelope serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct RedisStreamsAdapter {
    client: redis::Client,
    config: RedisStreamsConfig,
}

impl RedisStreamsAdapter {
    pub fn new(config: RedisStreamsConfig) -> Result<Self, RedisStreamError> {
        if config.redis_url.trim().is_empty()
            || config.stream_key.trim().is_empty()
            || config.consumer_group.trim().is_empty()
            || config.consumer_name.trim().is_empty()
        {
            return Err(RedisStreamError::InvalidConfiguration(
                "redis_url, stream_key, consumer_group, and consumer_name must not be blank"
                    .to_owned(),
            ));
        }

        if config.idempotency_ttl_secs == 0 || config.max_stream_len == 0 {
            return Err(RedisStreamError::InvalidConfiguration(
                "idempotency_ttl_secs and max_stream_len must be greater than zero".to_owned(),
            ));
        }

        let client = redis::Client::open(config.redis_url.clone())?;

        Ok(Self { client, config })
    }

    async fn connection(&self) -> Result<redis::aio::MultiplexedConnection, RedisStreamError> {
        Ok(self.client.get_multiplexed_async_connection().await?)
    }

    fn parse_stream_reply(
        reply: redis::streams::StreamReadReply,
    ) -> Result<Vec<StreamMessage>, RedisStreamError> {
        let mut events = Vec::new();
        for key in reply.keys {
            for item in key.ids {
                let raw: Option<String> = item.get("envelope");
                if let Some(raw) = raw {
                    let envelope: EventEnvelope = serde_json::from_str(&raw)?;
                    events.push(StreamMessage {
                        stream_id: item.id,
                        envelope,
                    });
                }
            }
        }

        Ok(events)
    }

    fn validate_ack_count(stream_id: &str, ack_count: i64) -> Result<(), RedisStreamError> {
        if ack_count == 1 {
            return Ok(());
        }

        Err(RedisStreamError::UnexpectedAckCount {
            stream_id: stream_id.to_owned(),
            actual: ack_count,
        })
    }

    pub async fn ensure_consumer_group(&self) -> Result<(), RedisStreamError> {
        let mut conn = self.connection().await?;
        let result: redis::RedisResult<()> = redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(&self.config.stream_key)
            .arg(&self.config.consumer_group)
            .arg("0")
            .arg("MKSTREAM")
            .query_async(&mut conn)
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(error) if error.code() == Some("BUSYGROUP") => Ok(()),
            Err(error) => Err(RedisStreamError::ConsumerGroupInitialization {
                code: error.code().unwrap_or("UNKNOWN").to_owned(),
                detail: error.to_string(),
            }),
        }
    }

    pub async fn publish(&self, envelope: &EventEnvelope) -> Result<String, RedisStreamError> {
        let mut conn = self.connection().await?;
        let payload = serde_json::to_string(envelope)?;

        let stream_id: String = redis::cmd("XADD")
            .arg(&self.config.stream_key)
            .arg("MAXLEN")
            .arg("~")
            .arg(self.config.max_stream_len)
            .arg("*")
            .arg("envelope")
            .arg(payload)
            .query_async(&mut conn)
            .await?;

        Ok(stream_id)
    }

    pub async fn read_group(
        &self,
        count: usize,
        block_ms: usize,
    ) -> Result<Vec<StreamMessage>, RedisStreamError> {
        let mut conn = self.connection().await?;
        let pending_options = StreamReadOptions::default()
            .group(&self.config.consumer_group, &self.config.consumer_name)
            .count(count);
        let pending_reply: redis::streams::StreamReadReply = conn
            .xread_options(&[&self.config.stream_key], &["0"], &pending_options)
            .await?;
        let pending = Self::parse_stream_reply(pending_reply)?;
        if !pending.is_empty() {
            return Ok(pending);
        }

        let fresh_options = StreamReadOptions::default()
            .group(&self.config.consumer_group, &self.config.consumer_name)
            .count(count)
            .block(block_ms);

        let fresh_reply: redis::streams::StreamReadReply = conn
            .xread_options(&[&self.config.stream_key], &[">"], &fresh_options)
            .await?;

        Self::parse_stream_reply(fresh_reply)
    }

    pub async fn ack(&self, stream_id: &str) -> Result<(), RedisStreamError> {
        let mut conn = self.connection().await?;
        let ack_count: i64 = redis::cmd("XACK")
            .arg(&self.config.stream_key)
            .arg(&self.config.consumer_group)
            .arg(stream_id)
            .query_async(&mut conn)
            .await?;

        Self::validate_ack_count(stream_id, ack_count)
    }

    pub async fn mark_processed(&self, idempotency_key: &str) -> Result<(), RedisStreamError> {
        let mut conn = self.connection().await?;
        let key = format!("processed:{}", idempotency_key);

        conn.set_ex::<_, _, ()>(key, "1", self.config.idempotency_ttl_secs)
            .await?;
        Ok(())
    }

    pub async fn already_processed(&self, idempotency_key: &str) -> Result<bool, RedisStreamError> {
        let mut conn = self.connection().await?;
        let key = format!("processed:{}", idempotency_key);
        let exists: i64 = redis::cmd("EXISTS").arg(key).query_async(&mut conn).await?;
        Ok(exists > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_envelope_new_sets_defaults() {
        let envelope =
            EventEnvelope::new("graph.rebuilt", "graph.rebuilt:42", serde_json::json!({}));

        assert_eq!(envelope.event_type, "graph.rebuilt");
        assert_eq!(envelope.idempotency_key, "graph.rebuilt:42");
        assert_eq!(envelope.schema_version, 1);
    }

    #[test]
    fn redis_config_validation_rejects_blank_values() {
        let mut config = RedisStreamsConfig::default();
        config.stream_key = " ".to_owned();

        let error = RedisStreamsAdapter::new(config)
            .expect_err("blank stream_key should fail config validation");

        assert!(matches!(error, RedisStreamError::InvalidConfiguration(_)));
    }

    #[test]
    fn ack_validation_requires_single_ack() {
        let error = RedisStreamsAdapter::validate_ack_count("123-0", 0)
            .expect_err("ack count 0 must be rejected");

        assert!(matches!(
            error,
            RedisStreamError::UnexpectedAckCount { stream_id, actual }
                if stream_id == "123-0" && actual == 0
        ));
    }
}
