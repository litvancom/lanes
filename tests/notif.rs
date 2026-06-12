//! UserNotifRegistry tests (RT-04 / 06-05).
//!
//! Tests for per-user notification channel delivery and isolation.

#[cfg(test)]
mod tests {
    use lanes::server::user_notif_registry::UserNotifRegistry;
    use lanes::models::events::NotifEvent;
    use tokio::sync::mpsc::error::TryRecvError;

    /// A published event reaches the subscribed user (delivery).
    #[tokio::test]
    async fn test_mention_notif_delivery() {
        let reg = UserNotifRegistry::new();
        let mut rx = reg.subscribe("userB");

        let event = NotifEvent::UnreadCountUpdated { count: 1 };
        reg.publish("userB", event.clone());

        match rx.try_recv() {
            Ok(NotifEvent::UnreadCountUpdated { count }) => {
                assert_eq!(count, 1, "delivered count should be 1");
            }
            Ok(other) => panic!("expected UnreadCountUpdated, got {other:?}"),
            Err(e) => panic!("expected event, got {e:?}"),
        }
    }

    /// Publishing to userB does NOT deliver to userA (isolation).
    #[tokio::test]
    async fn test_mention_notif_isolation() {
        let reg = UserNotifRegistry::new();
        let mut rx_a = reg.subscribe("userA");
        let mut rx_b = reg.subscribe("userB");

        // Publish only to userB.
        let event = NotifEvent::UnreadCountUpdated { count: 3 };
        reg.publish("userB", event);

        // userA should receive nothing.
        match rx_a.try_recv() {
            Err(TryRecvError::Empty) => {} // correct — userA is isolated
            Ok(ev) => panic!("userA should NOT receive anything, but got {ev:?}"),
            Err(e) => panic!("unexpected error on userA rx: {e:?}"),
        }

        // userB should receive the event.
        match rx_b.try_recv() {
            Ok(NotifEvent::UnreadCountUpdated { count }) => {
                assert_eq!(count, 3);
            }
            Ok(other) => panic!("expected UnreadCountUpdated for userB, got {other:?}"),
            Err(e) => panic!("userB should receive event, got {e:?}"),
        }
    }

    /// After remove(), publishing no longer panics and the channel is gone.
    #[tokio::test]
    async fn test_notif_cleanup_on_remove() {
        let reg = UserNotifRegistry::new();
        let _rx = reg.subscribe("userB");

        // Remove the user's channel (simulates WS disconnect).
        reg.remove("userB");

        // Publishing to a removed user should NOT panic — it silently drops.
        reg.publish("userB", NotifEvent::UnreadCountUpdated { count: 99 });
        // If we got here without panic, the test passes.
    }
}
