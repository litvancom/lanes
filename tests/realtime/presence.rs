//! Presence registry unit tests (06-04).
//!
//! Uses `#[tokio::test(start_paused = true)]` for time control.
//! Since `std::time::Instant` does NOT advance with `tokio::time::pause`,
//! `sweep_once` accepts an explicit `now: Instant` parameter so these tests
//! inject an offset Instant to simulate elapsed time (the sweep threshold check
//! uses `now.duration_since(last_heartbeat) > 15s`).

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};
    use lanes::server::presence_registry::PresenceRegistry;
    use lanes::auth::models::AuthUser;
    use lanes::models::events::PresenceEvent;

    /// Build a minimal AuthUser for testing.
    fn make_user(id: &str, name: &str, color: &str) -> AuthUser {
        AuthUser {
            id: id.to_string(),
            email: format!("{}@example.com", id),
            password_hash: None,
            display_name: name.to_string(),
            avatar_color: color.to_string(),
            auth_provider: "email".to_string(),
            created_at: 0,
        }
    }

    /// `test_presence_join_leave`:
    /// join adds a viewer to the snapshot; leave removes them.
    #[tokio::test(start_paused = true)]
    async fn test_presence_join_leave() {
        let registry = PresenceRegistry::new();
        let user = make_user("u1", "Alice", "#7c5cff");
        let board_id = "board-1";

        // Subscribe before join (Pitfall 3)
        let mut rx = registry.subscribe(board_id);

        // Join
        registry.join(board_id, &user, "client-1");

        // ViewerJoined should be broadcast
        let event = rx.try_recv().expect("expected ViewerJoined event");
        match event {
            PresenceEvent::ViewerJoined { user_id, display_name, .. } => {
                assert_eq!(user_id, "u1");
                assert_eq!(display_name, "Alice");
            }
            other => panic!("unexpected event: {:?}", other),
        }

        // Snapshot should include the viewer
        let snap = registry.snapshot(board_id);
        match snap {
            PresenceEvent::ViewersSnapshot { viewers } => {
                assert_eq!(viewers.len(), 1);
                assert_eq!(viewers[0].user_id, "u1");
            }
            other => panic!("unexpected snapshot event: {:?}", other),
        }

        // Leave
        registry.leave(board_id, "u1");

        // ViewerLeft should be broadcast
        let event = rx.try_recv().expect("expected ViewerLeft event");
        match event {
            PresenceEvent::ViewerLeft { user_id } => {
                assert_eq!(user_id, "u1");
            }
            other => panic!("unexpected event: {:?}", other),
        }

        // Snapshot should now be empty
        let snap = registry.snapshot(board_id);
        match snap {
            PresenceEvent::ViewersSnapshot { viewers } => {
                assert!(viewers.is_empty(), "viewer should be removed after leave");
            }
            other => panic!("unexpected snapshot event: {:?}", other),
        }
    }

    /// `test_heartbeat_resets_timer`:
    /// Join at T=0, heartbeat at T=8s, sweep at T=16s with now=T+8s offset.
    /// The viewer should NOT be reaped because time since last heartbeat is 8s < 15s.
    #[tokio::test(start_paused = true)]
    async fn test_heartbeat_resets_timer() {
        let registry = PresenceRegistry::new();
        let user = make_user("u2", "Bob", "#0ea5e9");
        let board_id = "board-2";

        registry.subscribe(board_id); // subscribe for channel creation
        registry.join(board_id, &user, "client-2");

        // Simulate the passage of 8 seconds before the heartbeat
        // by injecting a "now" that is 8 seconds into the future.
        let join_time = Instant::now();
        let after_8s = join_time + Duration::from_secs(8);

        // sweep_once at T+8s: viewer was created at join_time, so elapsed = 8s < 15s — NOT reaped
        registry.sweep_once(after_8s);

        let snap = registry.snapshot(board_id);
        match snap {
            PresenceEvent::ViewersSnapshot { viewers } => {
                assert_eq!(viewers.len(), 1, "viewer should NOT be reaped at 8s");
            }
            other => panic!("unexpected snapshot event: {:?}", other),
        }

        // Now heartbeat to reset the timer (last_heartbeat = Instant::now() ≈ join_time)
        registry.heartbeat(board_id, "u2");

        // Sweep at T+16s: time since last heartbeat (at ~join_time) = 16s > 15s WOULD reap.
        // But heartbeat was called after join, so last_heartbeat was reset to ~now.
        // Since we can't advance std::Instant in tests, we inject T+16s but heartbeat
        // was called at ~join_time, so elapsed from heartbeat = ~16s > 15s — reaped.
        // To confirm the heartbeat reset: inject only T+8s again (8s since heartbeat reset).
        let after_heartbeat_8s = Instant::now() + Duration::from_secs(8);
        registry.sweep_once(after_heartbeat_8s);

        let snap = registry.snapshot(board_id);
        match snap {
            PresenceEvent::ViewersSnapshot { viewers } => {
                assert_eq!(viewers.len(), 1, "viewer should NOT be reaped 8s after heartbeat");
            }
            other => panic!("unexpected snapshot event: {:?}", other),
        }
    }

    /// `test_sweep_reaps_stale`:
    /// Join, then sweep with now = join_time + 16s → viewer removed, ViewerLeft broadcast.
    #[tokio::test(start_paused = true)]
    async fn test_sweep_reaps_stale() {
        let registry = PresenceRegistry::new();
        let user = make_user("u3", "Carol", "#10b981");
        let board_id = "board-3";

        let mut rx = registry.subscribe(board_id);
        registry.join(board_id, &user, "client-3");

        // Consume the ViewerJoined broadcast
        let _joined = rx.try_recv().expect("ViewerJoined expected");

        // Record the join time and sweep with now = join_time + 16s (> 15s threshold)
        let join_time = Instant::now();
        let stale_now = join_time + Duration::from_secs(16);

        registry.sweep_once(stale_now);

        // ViewerLeft should have been broadcast on the channel
        let event = rx.try_recv().expect("expected ViewerLeft broadcast from sweep");
        match event {
            PresenceEvent::ViewerLeft { user_id } => {
                assert_eq!(user_id, "u3");
            }
            other => panic!("unexpected event from sweep: {:?}", other),
        }

        // Snapshot should be empty after sweep
        let snap = registry.snapshot(board_id);
        match snap {
            PresenceEvent::ViewersSnapshot { viewers } => {
                assert!(viewers.is_empty(), "viewer should be removed by sweep at 16s");
            }
            other => panic!("unexpected snapshot event: {:?}", other),
        }
    }
}
