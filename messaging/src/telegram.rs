use domain::{Channel, ChannelIdentity};
use serde::Deserialize;
use crate::channel_trait::{http_like::Headers, ChannelAdapter, ChannelError, InboundMessage};

pub struct TelegramAdapter {
    secret_token: String,   // X-Telegram-Bot-Api-Secret-Token
    bot_token: String,      // for Bot API calls
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct Update {
    update_id: i64,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    from: Option<User>,
    text: Option<String>,
}

#[derive(Deserialize)]
struct User {
    id: i64,
}

impl TelegramAdapter {
    pub fn new(secret_token: impl Into<String>, bot_token: impl Into<String>) -> Self {
        Self {
            secret_token: secret_token.into(),
            bot_token: bot_token.into(),
            http: reqwest::Client::new(),
        }
    }

    /// One-time: register webhook URL with Telegram.
    pub async fn set_webhook(&self, url: &str) -> Result<(), ChannelError> {
        self.http
            .post(format!("https://api.telegram.org/bot{}/setWebhook", self.bot_token))
            .json(&serde_json::json!({ "url": url, "secret_token": self.secret_token }))
            .send()
            .await
            .map_err(|e| ChannelError::SendFailed(e.to_string()))?;
        Ok(())
    }
}

impl ChannelAdapter for TelegramAdapter {
    fn verify(&self, headers: &Headers, _raw_body: &[u8]) -> Result<(), ChannelError> {
        // R03: static string compare, not HMAC
        let tok = headers
            .get("x-telegram-bot-api-secret-token")
            .ok_or(ChannelError::VerificationFailed)?;
        if tok != self.secret_token {
            return Err(ChannelError::VerificationFailed);
        }
        Ok(())
    }

    fn parse_events(&self, raw_body: &[u8]) -> Result<Vec<InboundMessage>, ChannelError> {
        let update: Update = serde_json::from_slice(raw_body)
            .map_err(|e| ChannelError::ParseFailed(e.to_string()))?;

        let Some(msg) = update.message else { return Ok(vec![]); };
        let Some(from) = msg.from else { return Ok(vec![]); };
        let Some(text) = msg.text else { return Ok(vec![]); };

        Ok(vec![InboundMessage {
            sender: ChannelIdentity {
                channel: Channel::Telegram,
                external_id: from.id.to_string(),
            },
            text,
            // R03: update_id sequential+unique per bot -> safe dedup key
            external_event_id: update.update_id.to_string(),
            reply_token: None, // no reply-token concept
        }])
    }

    fn send_push(
        &self,
        recipient: &ChannelIdentity,
        text: &str,
    ) -> impl std::future::Future<Output = Result<(), ChannelError>> + Send {
        async move {
            let url = format!(
                "https://api.telegram.org/bot{}/sendMessage",
                self.bot_token
            );
            self.http
                .post(url)
                .json(&serde_json::json!({
                    "chat_id": recipient.external_id,
                    "text": text,
                }))
                .send()
                .await
                .map_err(|e| ChannelError::SendFailed(e.to_string()))?;
            Ok(())
        }
    }
}