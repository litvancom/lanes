//! Tests for the ConsoleMailer implementation.
//! Run: DATABASE_URL=sqlite://data/lanes.db cargo test --features ssr mailer_tests

#[cfg(feature = "ssr")]
mod mailer_tests {
    use lanes::mailer::{Mailer, MailerError};
    use lanes::mailer::console::ConsoleMailer;

    /// Test: ConsoleMailer::send_invite returns Ok and does not panic (COLLAB-02 floor).
    #[tokio::test]
    async fn test_console_mailer_send_invite_returns_ok() {
        let mailer = ConsoleMailer;
        let result = mailer.send_invite(
            "invitee@test.com",
            "/invite/abc123token",
            "My Test Board",
        ).await;
        assert!(result.is_ok(), "ConsoleMailer::send_invite should return Ok");
    }

    /// Test: ConsoleMailer can handle edge-case inputs without panicking.
    #[tokio::test]
    async fn test_console_mailer_handles_special_chars() {
        let mailer = ConsoleMailer;
        let result = mailer.send_invite(
            "user+tag@example.co.uk",
            "/invite/xyz-!token_test",
            "Board with 'quotes' & special <chars>",
        ).await;
        assert!(result.is_ok(), "ConsoleMailer should handle special chars gracefully");
    }
}
