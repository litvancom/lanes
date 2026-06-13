use leptos::prelude::*;
use leptos::form::ActionForm;
use crate::api::members_api::{list_board_members, MemberRow, SetMemberRole, RemoveMember};
use crate::components::modal::Modal;
use crate::components::invite_panel::InvitePanel;

/// ShareModal: board-sharing overlay (invite + member management).
///
/// Shows when `show` is true (delegates show/hide + backdrop/Escape to the Modal shell).
/// Top section: InvitePanel (invite by email with role select).
/// Bottom section: member list with per-member role change and remove (non-owners only).
///
/// The member list is driven by a Resource that calls `list_board_members` and refetches
/// whenever SetMemberRole or RemoveMember actions complete (via Effect on version()).
#[component]
pub fn ShareModal(
    /// The board ID whose members are shown / managed
    board_id: String,
    /// Signal controlling whether the modal is open
    show: RwSignal<bool>,
) -> impl IntoView {
    // Store board_id so it can be moved into multiple closures without borrowing issues.
    let board_id_sv = StoredValue::new(board_id);

    // Server actions for managing existing members.
    let set_role = ServerAction::<SetMemberRole>::new();
    let remove = ServerAction::<RemoveMember>::new();

    // Resource: fetch member list; refetch when set_role or remove complete.
    let members = Resource::new(
        move || (set_role.version().get(), remove.version().get()),
        move |_| {
            let bid = board_id_sv.get_value();
            async move { list_board_members(bid).await }
        },
    );

    view! {
        <Modal show=show>
            <h2 id="modal-heading" class="lns-modal-title">"Share board"</h2>

            // ── Invite section ─────────────────────────────────────────
            <InvitePanel board_id=board_id_sv.get_value() />

            // ── Members section ────────────────────────────────────────
            <div class="lns-share-members">
                <h3 class="lns-share-members-heading">"Members"</h3>

                <Suspense fallback=move || view! {
                    <p class="lns-share-members-loading">"Loading members…"</p>
                }>
                    {move || Suspend::new(async move {
                        match members.await {
                            Err(e) => view! {
                                <p class="lns-error-banner-text">{e.to_string()}</p>
                            }.into_any(),
                            Ok(rows) => {
                                rows.into_iter().map(|row: MemberRow| {
                                    // Clone per-row values so each closure is self-contained.
                                    let user_id = row.user_id.clone();
                                    let display_name = row.display_name.clone();
                                    let email = row.email.clone();
                                    let role = row.role.clone();
                                    let is_owner = role == "owner";

                                    let bid_for_role = board_id_sv.get_value();
                                    let uid_for_role = user_id.clone();

                                    let bid_for_remove = board_id_sv.get_value();
                                    let uid_for_remove = user_id.clone();

                                    view! {
                                        <div class="lns-share-member-row">
                                            // Name + email
                                            <div class="lns-share-member-info">
                                                <span class="lns-share-member-name">{display_name}</span>
                                                <span class="lns-share-member-email">{email}</span>
                                            </div>

                                            // Controls: owner gets a static label; others get role select + remove
                                            <div class="lns-share-member-controls">
                                                {if is_owner {
                                                    view! {
                                                        <span class="lns-share-member-owner-label">"Owner"</span>
                                                    }.into_any()
                                                } else {
                                                    view! {
                                                        // Role change form
                                                        <ActionForm action=set_role>
                                                            <input type="hidden" name="board_id" value=bid_for_role />
                                                            <input type="hidden" name="user_id" value=uid_for_role />
                                                            <select
                                                                name="role"
                                                                class="lns-select lns-select--inline"
                                                                // Submit on change for immediate UX
                                                                on:change=move |ev| {
                                                                    let _ = ev; // form submits via native submit
                                                                }
                                                            >
                                                                <option
                                                                    value="editor"
                                                                    selected={role.as_str() == "editor"}
                                                                >"Can edit"</option>
                                                                <option
                                                                    value="commenter"
                                                                    selected={role.as_str() == "commenter"}
                                                                >"Read + comment"</option>
                                                                <option
                                                                    value="viewer"
                                                                    selected={role.as_str() == "viewer"}
                                                                >"Read-only"</option>
                                                            </select>
                                                            <button
                                                                type="submit"
                                                                class="lns-btn lns-btn--ghost lns-btn--sm"
                                                                disabled=move || set_role.pending().get()
                                                            >
                                                                "Save"
                                                            </button>
                                                        </ActionForm>

                                                        // Remove form
                                                        <ActionForm action=remove>
                                                            <input type="hidden" name="board_id" value=bid_for_remove />
                                                            <input type="hidden" name="user_id" value=uid_for_remove />
                                                            <button
                                                                type="submit"
                                                                class="lns-btn lns-btn--ghost lns-btn--sm lns-btn--danger"
                                                                disabled=move || remove.pending().get()
                                                            >
                                                                "Remove"
                                                            </button>
                                                        </ActionForm>
                                                    }.into_any()
                                                }}
                                            </div>
                                        </div>
                                    }
                                }).collect_view()
                            }.into_any()
                        }
                    })}
                </Suspense>
            </div>
        </Modal>
    }
}
