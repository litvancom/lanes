//! Wave-0 broadcast registry tests.
//!
//! `test_broadcast_move_card` and `test_board_isolation` are filled here (Task 2 registry).
//! `test_publish_hooks` and `test_lagged_refresh` are placeholders for 06-02 and 06-03.

#[cfg(test)]
mod tests {
    use lanes::server::board_rooms::BoardRoomRegistry;
    use lanes::models::events::BoardEvent;

    /// Fan-out: two subscribers on the same board both receive a CardMoved event.
    #[tokio::test]
    async fn test_broadcast_move_card() {
        let registry = BoardRoomRegistry::new();
        let mut rx1 = registry.subscribe("board-1");
        let mut rx2 = registry.subscribe("board-1");

        let seq = registry.next_seq("board-1");
        registry.publish(
            "board-1",
            BoardEvent::CardMoved {
                board_seq: seq,
                client_id: "c-abc".to_string(),
                card_id: "card-1".to_string(),
                to_list_id: "list-2".to_string(),
                position: "a0".to_string(),
            },
        );

        let ev1 = rx1.recv().await.expect("rx1 should receive event");
        let ev2 = rx2.recv().await.expect("rx2 should receive event");

        match (&ev1, &ev2) {
            (
                BoardEvent::CardMoved { card_id: id1, board_seq: seq1, .. },
                BoardEvent::CardMoved { card_id: id2, board_seq: seq2, .. },
            ) => {
                assert_eq!(id1, "card-1");
                assert_eq!(id2, "card-1");
                assert_eq!(*seq1, seq);
                assert_eq!(*seq2, seq);
            }
            _ => panic!("Expected CardMoved events, got {ev1:?} / {ev2:?}"),
        }
    }

    /// Isolation: publishing to board A is not received by a subscriber of board B.
    #[tokio::test]
    async fn test_board_isolation() {
        let registry = BoardRoomRegistry::new();
        let _rx_a = registry.subscribe("board-A");
        let mut rx_b = registry.subscribe("board-B");

        registry.publish(
            "board-A",
            BoardEvent::CardMoved {
                board_seq: 1,
                client_id: "c-xyz".to_string(),
                card_id: "card-99".to_string(),
                to_list_id: "list-3".to_string(),
                position: "b0".to_string(),
            },
        );

        // Board B receiver should see nothing
        match rx_b.try_recv() {
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {} // correct
            other => panic!("Board B should not receive Board A events; got: {other:?}"),
        }
    }

    /// Placeholder for publish hook integration tests (filled by 06-02).
    /// Asserts BoardEvent variants serialize/deserialize round-trip correctly.
    #[test]
    fn test_publish_hooks() {
        // filled in by 06-02
        assert!(true);
    }

    /// Placeholder for lagged receiver → Refresh behavior (filled by 06-03).
    #[tokio::test]
    async fn test_lagged_refresh() {
        // filled in by 06-03
        assert!(true);
    }
}
