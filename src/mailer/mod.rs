/// Pluggable mailer abstraction (COLLAB-02 floor, D-13).
/// Console transport is the only implementation this phase; SMTP (lettre) deferred to Phase 7.
pub mod console;

/// Mailer errors.
#[derive(Debug, thiserror::Error)]
pub enum MailerError {
    #[error("Mail delivery error: {0}")]
    Delivery(String),
}

/// Pluggable mailer trait. All implementations must be Send + Sync for Arc<dyn Mailer> usage.
#[async_trait::async_trait]
pub trait Mailer: Send + Sync {
    async fn send_invite(
        &self,
        to_email: &str,
        invite_url: &str,
        board_name: &str,
    ) -> Result<(), MailerError>;
}
