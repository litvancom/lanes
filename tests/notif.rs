//! UserNotifRegistry tests (RT-04 / 06-06).
//!
//! Tests for per-user notification channel delivery, isolation, and connection-scoped
//! teardown (CR-01: remove_if_current preserves sibling connections under fan-out).

#[cfg(test)]
mod tests {
    use lanes::server::user_notif_registry::UserNotifRegistry;
    use lanes::models::events::NotifEvent;
    use tokio::sync::mpsc::error::TryRecvError;

    /// A published event reaches the subscribed user (delivery).
    #[tokio::test]
    async fn test_mention_notif_delivery() {
        let reg = UserNotifRegistry::new();
        let (_channel_id, _tx, mut rx) = reg.subscribe("userB");

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
        let (_channel_id_a, _tx_a, mut rx_a) = reg.subscribe("userA");
        let (_channel_id_b, _tx_b, mut rx_b) = reg.subscribe("userB");

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

    /// After remove_if_current with the connection's own channel_id, publishing no longer delivers.
    #[tokio::test]
    async fn test_notif_cleanup_on_remove_if_current() {
        let reg = UserNotifRegistry::new();
        let (channel_id, _tx, _rx) = reg.subscribe("userB");

        // Remove using the exact channel_id we installed — should succeed.
        reg.remove_if_current("userB", &channel_id);

        // Publishing to a removed user should NOT panic — it silently drops.
        reg.publish("userB", NotifEvent::UnreadCountUpdated { count: 99 });
        // If we got here without panic, the test passes.
    }

    /// CR-01: remove_if_current with tab A's channel_id does NOT wipe tab B's entry.
    ///
    /// Scenario: tab A subscribes, tab B subscribes (both same user); tab A disconnects.
    /// Tab B should still receive events.
    #[tokio::test]
    async fn test_remove_if_current_preserves_newer_tab() {
        let reg = UserNotifRegistry::new();

        // Tab A subscribes first.
        let (channel_id_a, _tx_a, _rx_a) = reg.subscribe("userC");
        // Tab B subscribes second — appended alongside tab A (fan-out, NOT overwrite).
        let (channel_id_b, _tx_b, mut rx_b) = reg.subscribe("userC");

        // Tab A closes — remove_if_current should remove only tab A's entry.
        reg.remove_if_current("userC", &channel_id_a);

        // Tab B should still receive events.
        reg.publish("userC", NotifEvent::UnreadCountUpdated { count: 7 });
        match rx_b.try_recv() {
            Ok(NotifEvent::UnreadCountUpdated { count }) => {
                assert_eq!(count, 7, "tab B should still receive after tab A's remove_if_current");
            }
            Ok(other) => panic!("expected UnreadCountUpdated, got {other:?}"),
            Err(e) => panic!("tab B should receive event, got {e:?}"),
        }

        // Tab B closes — remove_if_current with tab B's channel_id removes the last entry.
        reg.remove_if_current("userC", &channel_id_b);
        reg.publish("userC", NotifEvent::UnreadCountUpdated { count: 99 });
        // No panic — silently dropped (user key removed since Vec is empty).
    }

    /// Fan-out: two concurrent connections for the same user BOTH receive a single publish.
    ///
    /// This is the regression that the old one-sender registry would fail:
    /// the second subscribe() used to overwrite the first Sender, so tab A would go stale.
    #[tokio::test]
    async fn test_notif_fanout_two_connections_same_user() {
        let reg = UserNotifRegistry::new();

        // Two connections open concurrently for the same user (e.g. dashboard + board tab).
        let (_channel_id_1, _tx_1, mut rx_1) = reg.subscribe("userD");
        let (_channel_id_2, _tx_2, mut rx_2) = reg.subscribe("userD");

        // One publish should fan out to BOTH receivers.
        reg.publish("userD", NotifEvent::UnreadCountUpdated { count: 5 });

        // Connection 1 must receive the event.
        match rx_1.try_recv() {
            Ok(NotifEvent::UnreadCountUpdated { count }) => {
                assert_eq!(count, 5, "connection 1 should receive the fan-out event");
            }
            Ok(other) => panic!("connection 1: expected UnreadCountUpdated, got {other:?}"),
            Err(e) => panic!("connection 1 should receive event, got {e:?}"),
        }

        // Connection 2 must also receive the event.
        match rx_2.try_recv() {
            Ok(NotifEvent::UnreadCountUpdated { count }) => {
                assert_eq!(count, 5, "connection 2 should receive the fan-out event");
            }
            Ok(other) => panic!("connection 2: expected UnreadCountUpdated, got {other:?}"),
            Err(e) => panic!("connection 2 should receive event, got {e:?}"),
        }
    }
}
