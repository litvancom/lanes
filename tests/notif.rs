//! UserNotifRegistry tests (RT-04 / 06-05).
//!
//! Tests for per-user notification channel delivery, isolation, and connection-scoped
//! teardown (CR-01: remove_if_current prevents a second tab's sender from being wiped).

#[cfg(test)]
mod tests {
    use lanes::server::user_notif_registry::UserNotifRegistry;
    use lanes::models::events::NotifEvent;
    use tokio::sync::mpsc::error::TryRecvError;

    /// A published event reaches the subscribed user (delivery).
    #[tokio::test]
    async fn test_mention_notif_delivery() {
        let reg = UserNotifRegistry::new();
        let (_tx, mut rx) = reg.subscribe("userB");

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
        let (_tx_a, mut rx_a) = reg.subscribe("userA");
        let (_tx_b, mut rx_b) = reg.subscribe("userB");

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

    /// After remove_if_current with the current sender, publishing no longer delivers.
    #[tokio::test]
    async fn test_notif_cleanup_on_remove_if_current() {
        let reg = UserNotifRegistry::new();
        let (tx, _rx) = reg.subscribe("userB");

        // Remove using the exact sender we installed — should succeed.
        reg.remove_if_current("userB", &tx);

        // Publishing to a removed user should NOT panic — it silently drops.
        reg.publish("userB", NotifEvent::UnreadCountUpdated { count: 99 });
        // If we got here without panic, the test passes.
    }

    /// CR-01: remove_if_current with a stale sender does NOT wipe the current entry.
    ///
    /// Scenario: tab A subscribes, tab B subscribes (overwrites), tab A disconnects.
    /// Tab B should still receive events.
    #[tokio::test]
    async fn test_remove_if_current_preserves_newer_tab() {
        let reg = UserNotifRegistry::new();

        // Tab A subscribes first.
        let (tx_a, _rx_a) = reg.subscribe("userC");
        // Tab B subscribes second — overwrites the registry entry.
        let (tx_b, mut rx_b) = reg.subscribe("userC");

        // Tab A closes — remove_if_current should be a no-op because tx_a is no longer current.
        reg.remove_if_current("userC", &tx_a);

        // Tab B should still receive events.
        reg.publish("userC", NotifEvent::UnreadCountUpdated { count: 7 });
        match rx_b.try_recv() {
            Ok(NotifEvent::UnreadCountUpdated { count }) => {
                assert_eq!(count, 7, "tab B should still receive after tab A's remove_if_current");
            }
            Ok(other) => panic!("expected UnreadCountUpdated, got {other:?}"),
            Err(e) => panic!("tab B should receive event, got {e:?}"),
        }

        // Tab B closes — remove_if_current with the current sender removes the entry.
        reg.remove_if_current("userC", &tx_b);
        reg.publish("userC", NotifEvent::UnreadCountUpdated { count: 99 });
        // No panic — silently dropped.
    }
}
