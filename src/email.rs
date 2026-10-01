use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::valid_single_email;

// Three form values contain at most 2,520 characters. Four UTF-8 bytes per
// character and three bytes per percent-encoded byte need <30.3 KiB including
// field names and separators, so 32 KiB accepts every valid form but remains
// far below Axum's 256 KiB request cap.
pub const MAX_EMAIL_BODY_BYTES: usize = 32 * 1024;
pub const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
const DEFAULT_RESEND_ENDPOINT: &str = "https://api.resend.com/emails";
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Clone, Debug, Deserialize)]
pub struct EmailInput {
    pub recipient: String,
    pub subject: String,
    pub message: String,
}

impl EmailInput {
    /// Validates bounded, single-recipient email fields.
    ///
    /// # Errors
    ///
    /// Returns an actionable validation error for an invalid or oversized field.
    pub fn validate(&self) -> Result<(), EmailError> {
        if !valid_single_email(&self.recipient) {
            return Err(EmailError::InvalidInput(
                "Enter one valid recipient email address.".into(),
            ));
        }
        if self.subject.trim().is_empty()
            || self.subject.chars().count() > 200
            || self.subject.contains(['\r', '\n'])
        {
            return Err(EmailError::InvalidInput(
                "Subject is required and must be 200 characters or fewer.".into(),
            ));
        }
        if self.message.chars().count() > 2_000 {
            return Err(EmailError::InvalidInput(
                "Message must be 2,000 characters or fewer.".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct ResendClient {
    client: reqwest::Client,
    api_key: String,
    from: String,
    endpoint: String,
}

#[derive(Debug, Error)]
pub enum EmailError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("The generated PDF is too large to email. Download it and send it manually.")]
    AttachmentTooLarge,
    #[error(
        "delivery unknown: the email provider did not answer before the timeout; the saved document is safe. Check Resend before retrying."
    )]
    DeliveryUnknown,
    #[error(
        "Resend did not accept the email. The saved document is safe; review the fields and retry."
    )]
    ProviderRejected,
    #[error("The email provider could not be reached. The saved document is safe; retry shortly.")]
    Transport,
    #[error(
        "Resend returned an invalid confirmation. The saved document is safe; retry after checking Resend."
    )]
    InvalidConfirmation,
}

#[derive(Serialize)]
struct SendRequest<'a> {
    from: &'a str,
    to: [&'a str; 1],
    subject: &'a str,
    text: &'a str,
    attachments: [Attachment; 1],
}

#[derive(Serialize)]
struct Attachment {
    filename: String,
    content: String,
}

#[derive(Deserialize)]
struct SendResponse {
    id: String,
}

impl ResendClient {
    /// Builds a bounded-time client for the Resend email endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTPS client cannot be constructed.
    pub fn new(api_key: String, from: String) -> Result<Self, EmailError> {
        Self::with_endpoint(api_key, from, DEFAULT_RESEND_ENDPOINT, DELIVERY_TIMEOUT)
    }

    pub(crate) fn with_endpoint(
        api_key: String,
        from: String,
        endpoint: &str,
        timeout: Duration,
    ) -> Result<Self, EmailError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|_| EmailError::Transport)?;
        Ok(Self {
            client,
            api_key,
            from,
            endpoint: endpoint.to_owned(),
        })
    }

    /// Sends one PDF attachment synchronously and returns Resend's message ID.
    ///
    /// # Errors
    ///
    /// Returns a validation, size, timeout, transport, provider, or confirmation error.
    pub async fn send_pdf(
        &self,
        input: &EmailInput,
        filename: &str,
        pdf_bytes: &[u8],
    ) -> Result<String, EmailError> {
        input.validate()?;
        if pdf_bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(EmailError::AttachmentTooLarge);
        }
        let request = SendRequest {
            from: &self.from,
            to: [input.recipient.trim()],
            subject: input.subject.trim(),
            text: input.message.trim(),
            attachments: [Attachment {
                filename: filename.to_owned(),
                content: STANDARD.encode(pdf_bytes),
            }],
        };
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    EmailError::DeliveryUnknown
                } else {
                    EmailError::Transport
                }
            })?;
        if !response.status().is_success() {
            return Err(EmailError::ProviderRejected);
        }
        let confirmation: SendResponse = response
            .json()
            .await
            .map_err(|_| EmailError::InvalidConfirmation)?;
        if confirmation.id.trim().is_empty() || confirmation.id.chars().count() > 200 {
            return Err(EmailError::InvalidConfirmation);
        }
        Ok(confirmation.id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
    use serde_json::Value;

    use super::*;

    #[derive(Clone)]
    struct MockState {
        response: StatusCode,
        body: &'static str,
        delay: Duration,
        received: Arc<Mutex<Option<Value>>>,
    }

    async fn mock_handler(
        State(state): State<MockState>,
        Json(body): Json<Value>,
    ) -> impl axum::response::IntoResponse {
        if let Ok(mut received) = state.received.lock() {
            *received = Some(body);
        }
        tokio::time::sleep(state.delay).await;
        (state.response, state.body)
    }

    async fn mock(state: MockState) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("{error}"));
        tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                Router::new()
                    .route("/emails", post(mock_handler))
                    .with_state(state),
            )
            .await;
        });
        format!("http://{address}/emails")
    }

    fn input() -> EmailInput {
        EmailInput {
            recipient: "customer@example.test".into(),
            subject: "Quote Q-1".into(),
            message: "Attached.".into(),
        }
    }

    #[test]
    fn validation_rejects_multiple_malformed_and_header_injection_addresses() {
        for recipient in [
            "missing-domain",
            "missing@domain",
            "one@example.test,two@example.test",
            "one@example.test\r\nBcc:two@example.test",
        ] {
            let mut value = input();
            value.recipient = recipient.into();
            assert!(matches!(value.validate(), Err(EmailError::InvalidInput(_))));
        }
        let mut boundary = input();
        boundary.recipient = format!("{}@example.test", "a".repeat(64));
        assert!(boundary.validate().is_ok());
    }

    #[tokio::test]
    async fn success_attaches_the_exact_pdf_bytes() {
        let received = Arc::new(Mutex::new(None));
        let endpoint = mock(MockState {
            response: StatusCode::OK,
            body: r#"{"id":"message-123"}"#,
            delay: Duration::ZERO,
            received: received.clone(),
        })
        .await;
        let client = ResendClient::with_endpoint(
            "secret".into(),
            "Shop <shop@example.test>".into(),
            &endpoint,
            Duration::from_secs(1),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let pdf = b"%PDF-exact-binary\0bytes";

        assert_eq!(
            client
                .send_pdf(&input(), "Q-1.pdf", pdf)
                .await
                .unwrap_or_default(),
            "message-123"
        );
        let body = received
            .lock()
            .ok()
            .and_then(|body| body.clone())
            .unwrap_or(Value::Null);
        let encoded = body["attachments"][0]["content"]
            .as_str()
            .unwrap_or_default();
        assert_eq!(STANDARD.decode(encoded).unwrap_or_default(), pdf);
    }

    #[tokio::test]
    async fn provider_failure_and_timeout_are_distinct_safe_errors() {
        let received = Arc::new(Mutex::new(None));
        let failure_endpoint = mock(MockState {
            response: StatusCode::BAD_REQUEST,
            body: "{}",
            delay: Duration::ZERO,
            received: received.clone(),
        })
        .await;
        let failure = ResendClient::with_endpoint(
            "secret".into(),
            "from@example.test".into(),
            &failure_endpoint,
            Duration::from_secs(1),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(
            failure.send_pdf(&input(), "Q.pdf", b"pdf").await,
            Err(EmailError::ProviderRejected)
        ));

        let timeout_endpoint = mock(MockState {
            response: StatusCode::OK,
            body: r#"{"id":"late"}"#,
            delay: Duration::from_millis(100),
            received,
        })
        .await;
        let timeout = ResendClient::with_endpoint(
            "secret".into(),
            "from@example.test".into(),
            &timeout_endpoint,
            Duration::from_millis(10),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(
            timeout.send_pdf(&input(), "Q.pdf", b"pdf").await,
            Err(EmailError::DeliveryUnknown)
        ));
        assert!(
            EmailError::DeliveryUnknown
                .to_string()
                .contains("delivery unknown")
        );
    }
}
