use crate::mailer::{Mailer, MailerError};

/// Console/log transport: logs the invite URL to stdout (D-13 dev floor).
/// Never logs secrets — only the public invite URL is logged.
/// The invite link is always functional regardless of email delivery (D-13).
pub struct ConsoleMailer;

#[async_trait::async_trait]
impl Mailer for ConsoleMailer {
    async fn send_invite(
        &self,
        to_email: &str,
        invite_url: &str,
        board_name: &str,
    ) -> Result<(), MailerError> {
        tracing::info!(
            target: "lanes::mailer",
            %to_email,
            %invite_url,
            %board_name,
            "INVITE (console transport): send this link to the recipient"
        );
        Ok(())
    }
}
