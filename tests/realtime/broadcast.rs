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

    /// Asserts every BoardEvent mutation variant serializes and deserializes round-trip
    /// correctly via serde_json. This guards the wire contract: if a variant field name
    /// changes, this test will catch it before 06-04/06-05 can silently break.
    #[test]
    fn test_publish_hooks() {
        use lanes::models::events::{BoardEvent, CardPatch, CardSummary};
        use lanes::models::CardLabel;

        fn rt(ev: BoardEvent) -> BoardEvent {
            let json = serde_json::to_string(&ev).expect("serialize");
            serde_json::from_str::<BoardEvent>(&json).expect("deserialize")
        }

        // CardAdded
        let ev = BoardEvent::CardAdded {
            board_seq: 1,
            client_id: "c-1".into(),
            card: CardSummary {
                id: "card-1".into(),
                list_id: "list-1".into(),
                board_id: "board-1".into(),
                card_num: 1,
                title: "Test".into(),
                position: "a0".into(),
                priority: None,
                due_at: None,
                done: false,
                cover: None,
                labels: vec![],
                member_ids: vec![],
            },
        };
        let rt_ev = rt(ev.clone());
        match rt_ev {
            BoardEvent::CardAdded { board_seq, client_id, card } => {
                assert_eq!(board_seq, 1);
                assert_eq!(client_id, "c-1");
                assert_eq!(card.title, "Test");
            }
            _ => panic!("CardAdded round-trip failed"),
        }

        // CardUpdated
        let ev = BoardEvent::CardUpdated {
            board_seq: 2,
            client_id: "c-2".into(),
            card_id: "card-1".into(),
            patch: CardPatch { title: Some("Updated".into()), description: None, cover: None, done: None, card_num: None },
        };
        let rt_ev = rt(ev);
        match rt_ev {
            BoardEvent::CardUpdated { patch, .. } => assert_eq!(patch.title.as_deref(), Some("Updated")),
            _ => panic!("CardUpdated round-trip failed"),
        }

        // CardArchived
        let ev = BoardEvent::CardArchived { board_seq: 3, client_id: "c-3".into(), card_id: "card-2".into() };
        match rt(ev) {
            BoardEvent::CardArchived { card_id, .. } => assert_eq!(card_id, "card-2"),
            _ => panic!("CardArchived round-trip failed"),
        }

        // CommentAdded
        let ev = BoardEvent::CommentAdded {
            board_seq: 4, client_id: "c-4".into(), card_id: "card-1".into(),
            comment_id: "cmt-1".into(), author_id: "user-1".into(),
            text: "Hello".into(), created_at: 1234567890,
        };
        match rt(ev) {
            BoardEvent::CommentAdded { text, .. } => assert_eq!(text, "Hello"),
            _ => panic!("CommentAdded round-trip failed"),
        }

        // ChecklistUpdated
        let ev = BoardEvent::ChecklistUpdated {
            board_seq: 5, client_id: "c-5".into(), card_id: "card-1".into(),
            checklist_done: 2, checklist_total: 3,
        };
        match rt(ev) {
            BoardEvent::ChecklistUpdated { checklist_done, checklist_total, .. } => {
                assert_eq!(checklist_done, 2);
                assert_eq!(checklist_total, 3);
            }
            _ => panic!("ChecklistUpdated round-trip failed"),
        }

        // LabelChanged
        let ev = BoardEvent::LabelChanged {
            board_seq: 6, client_id: "c-6".into(), card_id: "card-1".into(),
            labels: vec![CardLabel { id: "lbl-1".into(), name: "Bug".into(), color: "#f00".into() }],
        };
        match rt(ev) {
            BoardEvent::LabelChanged { labels, .. } => assert_eq!(labels.len(), 1),
            _ => panic!("LabelChanged round-trip failed"),
        }

        // PriorityChanged
        let ev = BoardEvent::PriorityChanged {
            board_seq: 7, client_id: "c-7".into(), card_id: "card-1".into(), priority: Some("P1".into()),
        };
        match rt(ev) {
            BoardEvent::PriorityChanged { priority, .. } => assert_eq!(priority.as_deref(), Some("P1")),
            _ => panic!("PriorityChanged round-trip failed"),
        }

        // DueDateChanged
        let ev = BoardEvent::DueDateChanged {
            board_seq: 8, client_id: "c-8".into(), card_id: "card-1".into(), due_at: Some(1234567890),
        };
        match rt(ev) {
            BoardEvent::DueDateChanged { due_at, .. } => assert_eq!(due_at, Some(1234567890)),
            _ => panic!("DueDateChanged round-trip failed"),
        }

        // MemberChanged
        let ev = BoardEvent::MemberChanged {
            board_seq: 9, client_id: "c-9".into(), card_id: "card-1".into(),
            member_ids: vec!["user-1".into()],
        };
        match rt(ev) {
            BoardEvent::MemberChanged { member_ids, .. } => assert_eq!(member_ids.len(), 1),
            _ => panic!("MemberChanged round-trip failed"),
        }

        // ListAdded
        let ev = BoardEvent::ListAdded {
            board_seq: 10, client_id: "c-10".into(), list_id: "list-1".into(),
            name: "To Do".into(), position: "b0".into(),
        };
        match rt(ev) {
            BoardEvent::ListAdded { name, .. } => assert_eq!(name, "To Do"),
            _ => panic!("ListAdded round-trip failed"),
        }

        // ListRenamed
        let ev = BoardEvent::ListRenamed {
            board_seq: 11, client_id: "c-11".into(), list_id: "list-1".into(), name: "In Progress".into(),
        };
        match rt(ev) {
            BoardEvent::ListRenamed { name, .. } => assert_eq!(name, "In Progress"),
            _ => panic!("ListRenamed round-trip failed"),
        }

        // ListReordered
        let ev = BoardEvent::ListReordered {
            board_seq: 12, client_id: "c-12".into(), list_id: "list-1".into(), position: "c0".into(),
        };
        match rt(ev) {
            BoardEvent::ListReordered { position, .. } => assert_eq!(position, "c0"),
            _ => panic!("ListReordered round-trip failed"),
        }

        // CardMovedCrossBoard
        let ev = BoardEvent::CardMovedCrossBoard {
            board_seq: 13, client_id: "c-13".into(), card_id: "card-5".into(),
        };
        match rt(ev) {
            BoardEvent::CardMovedCrossBoard { card_id, .. } => assert_eq!(card_id, "card-5"),
            _ => panic!("CardMovedCrossBoard round-trip failed"),
        }
    }

    /// Proves the slow-client lag condition: when a receiver falls more than 256 messages
    /// behind (the broadcast channel capacity), tokio returns `RecvError::Lagged`.
    ///
    /// This is the unit-level guard for the Lagged→Refresh path (RT-02, Pitfall 2 §749-755):
    /// the server-side WS handler converts Lagged into a `Refresh` event so the client
    /// performs a full board re-fetch rather than applying stale deltas.
    #[tokio::test]
    async fn test_lagged_refresh() {
        use tokio::sync::broadcast::error::RecvError;

        let registry = BoardRoomRegistry::new();

        // Subscribe but do NOT drain — this receiver will fall behind.
        let mut rx = registry.subscribe("board-lag");

        // Publish more than the channel capacity (256) without draining the receiver.
        // Each publish increments the per-board sequence number to produce distinct events.
        for _ in 0..=256 {
            let seq = registry.next_seq("board-lag");
            registry.publish(
                "board-lag",
                BoardEvent::CardMoved {
                    board_seq: seq,
                    client_id: "c-lag".to_string(),
                    card_id: "card-lag".to_string(),
                    to_list_id: "list-1".to_string(),
                    position: "a0".to_string(),
                },
            );
        }

        // The first recv() after falling behind should return Lagged.
        // tokio broadcast returns Err(RecvError::Lagged(n)) where n is the number of messages
        // skipped. The handler converts this to a Refresh sent over the WebSocket.
        let result = rx.recv().await;
        assert!(
            matches!(result, Err(RecvError::Lagged(_))),
            "Expected RecvError::Lagged after overflow, got: {result:?}"
        );
    }
}
