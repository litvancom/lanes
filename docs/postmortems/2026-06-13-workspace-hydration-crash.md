# Post-mortem: Workspace hydration crash (`RuntimeError: unreachable`)

**Date:** 2026-06-13
**Status:** Resolved (v0.2.7)
**Severity:** High — the primary page (`/`) was unusable after a hard refresh
**Author:** Engineering

---

## Summary

After a hard refresh of the workspace home page (`/`), the app crashed during
client-side hydration with `RuntimeError: unreachable` (panic in `tachys`
`hydration.rs:163`). The WASM runtime died, so the page froze: clicking a board
changed the URL but rendered nothing. A normal (soft) refresh "recovered" it.

The root cause was that the shared **`WorkspaceSidebar` read the blocking
`boards`/`starred` resources via a synchronous `.get()` outside any `<Suspense>`**.
Under concurrent SSR load the resource had not resolved when the sidebar rendered,
so the server emitted an **empty** board list while the client hydrated the
correctly-serialized **non-empty** value — a structural mismatch that panics
hydration.

Fixed by wrapping the sidebar's `<For>`/`<Show>` in `<Suspense>` so SSR waits for
the resource and renders its resolved value inline, matching the client.

---

## Impact

- **User impact:** Hard refresh of `/` (the landing page after login) crashed the
  SPA every time in production. Boards became unreachable until a soft refresh.
  Other routes (`/settings`, `/archive`, `/inbox`, `/calendar`) were not reported
  affected in practice, though they share the sidebar and the same latent bug.
- **Scope:** Production only. Never reproduced in local development until the
  concurrency conditions were deliberately recreated.
- **Data:** None. No data loss or corruption; purely a client-side render crash.

---

## Detection

Reported by a user: "sometimes I can't get board data… `Uncaught (in promise)
RuntimeError: unreachable`." Later refined to a precise, reproducible sequence:
**hard refresh of `/` breaks; soft refresh recovers; reproducible in incognito.**

---

## Root cause

The workspace created its board resources as **blocking** resources:

```rust
let boards  = Resource::new_blocking(|| (), |_| async { list_boards_with_meta().await });
let starred = Resource::new_blocking(|| (), |_| async { list_starred_boards().await });
```

and passed them to the sidebar as derived signals:

```rust
all_boards = Signal::derive(move || boards.get().and_then(|r| r.ok()).unwrap_or_default())
```

The sidebar then rendered them in a `<For>` **outside any `<Suspense>`**:

```rust
<div class="lns-sidebar-section">
    <h3>"BOARDS"</h3>
    <For each=move || all_boards.get() ... />   // synchronous read in the shell
</div>
```

Reading a resource with `.get()` outside a `<Suspense>` does **not** wait for it.
On a quiet request the blocking resource resolved before the sidebar rendered, so
`.get()` returned the data. **Under concurrent SSR load** (live notification
WebSockets + overlapping requests on the multi-core host), the resource had not
yet resolved at the sidebar's synchronous read → `.get()` returned `None` →
`unwrap_or_default()` → **empty `<For>` → zero board links in the server HTML**.

The resource's value was still serialized correctly into the hydration payload, so
the **client hydrated the real board links**. The server markup (0 links) did not
match the structure the client expected (N links) → `tachys` hydration walked into
an `unreachable` branch (it expected an `<a>` element and found the end of the
list / a comment marker) → panic → dead WASM runtime.

This is why it was **deterministic in production** (always enough concurrency from
the persistent WebSocket + cold-load requests) and **invisible locally** (a single
dev request always resolves the resource in time; server and browser also share a
clock, masking timezone red herrings).

---

## Timeline & the three wrong turns

The investigation took several iterations before the real cause was found. Each
wrong turn is recorded because the *reasoning* mattered:

| Ver | Change | Result |
|-----|--------|--------|
| 0.2.3 | Make workspace sidebar `boards`/`starred` **blocking** (renders links inline) | Necessary, but did not fix the crash |
| 0.2.4 | Same blocking fix for `/settings`, `/archive` | Did not fix the crash |
| 0.2.5 | Set authenticated routes to `SsrMode::Async` (eliminate streaming) | **Made it worse** — Async holds the reactive context across all awaits, amplifying a separate concurrent resource-serialization bleed |
| 0.2.6 | Revert `SsrMode::Async` | Removed the amplifier; crash still present |
| **0.2.7** | **Wrap sidebar `<For>`/`<Show>` in `<Suspense>`** | **Fixed — validated locally and in prod** |

Hypotheses ruled out *by reproduction or measurement*, not assumption: stale
browser cache / service worker, asset version skew, Traefik (reverse proxy)
transforms, debug-vs-release build differences, server/browser timezone mismatch,
and the streaming-vs-async SSR mode.

---

## How it was finally found

1. **Structural diff of SSR HTML.** Comparing a tag/comment skeleton of prod's
   root HTML against a known-good local one showed the divergence: prod rendered
   the sidebar `BOARDS` section as `<h3>…</h3>` followed immediately by `</div>` —
   **zero `<a>` links** — while local rendered the links.
2. **Local reproduction harness.** Opening ~6 `new WebSocket('/ws/notifications')`
   connections and firing ~50 parallel `fetch('/')` requests reproduced it:
   **22 of 23 responses had an empty sidebar.** A single or sequential request
   never triggered it.
3. **Validation.** After wrapping the sidebar lists in `<Suspense>`, the same
   harness produced **0 of 50** empty sidebars, locally and in production.

---

## Resolution

Wrap the sidebar's board and starred lists in `<Suspense>` so SSR **waits** for the
blocking resource and renders the resolved value inline (blocking resources render
in-order, not streamed), guaranteeing SSR and hydrate agree:

```rust
<div class="lns-sidebar-section">
    <h3>"BOARDS"</h3>
    <Suspense fallback=|| ()>
        <For each=move || all_boards.get() ... />
    </Suspense>
</div>
```

Fix lives in the shared `WorkspaceSidebar`, so it covers all sidebar-bearing
routes at once. Deployed in v0.2.7; verified by hard-refresh and a 50-concurrent /
6-WebSocket load test (0 empty sidebars, memory flat at ~8 MiB, ~746 req/s).

---

## What went well

- Refused to ship a fix without a reproduction once the early guesses failed.
- Built a quantitative reproduction/validation harness (`emptySidebar` count) that
  turned a vague "it crashes" into a number to drive to zero.
- Structural SSR-HTML diffing pinpointed the exact divergent element.

## What went wrong

- Three production deploys (0.2.3–0.2.5) were shipped on reasoning before the bug
  was reproduced; one (`SsrMode::Async`) regressed behavior.
- The release build's bare `unreachable` (panic messages stripped) hid the element
  name that the debug build prints — early effort was spent without it.
- Time was lost on a real-but-secondary phenomenon (concurrent resource-serialization
  bleed, amplified by `SsrMode::Async`) before isolating the actual crash trigger.

---

## Lessons learned

1. **Never read a resource for structural content outside `<Suspense>`/`<Transition>`.**
   A synchronous `.get()` in the shell races the resource's async resolution; under
   concurrency it returns `None` and the SSR/hydrate structures diverge. This is the
   single most important takeaway and now lives in project memory.
2. **Concurrency bugs need concurrency to reproduce.** Sequential local requests
   will not surface a race that only appears under parallel load + long-lived
   WebSocket connections. Reproduce with the real concurrency shape.
3. **Reproduce before deploying.** A locally-validated harness (here:
   `responsesWithEmptySidebar`) is worth more than several plausible-but-unverified
   production deploys.
4. **Production-only ≠ environmental.** "Only in prod" pointed at infra (Traefik,
   timezone, build mode) for a long time; it was actually concurrency that local
   dev simply never generated.

---

## Action items

- [x] Fix the sidebar (`<Suspense>` wrap) — v0.2.7.
- [x] Record the root-cause rule in project memory.
- [ ] Audit all other resources read outside `<Suspense>` in the shell (board
      header, topbar, any future shell content) and apply the same pattern.
- [ ] Add a CI/load smoke test that fires concurrent authenticated requests and
      asserts the sidebar renders its links (guards against regressions of this class).
- [ ] Consider adding modest pod `requests`/`limits` to the Helm chart as a
      guardrail (memory is currently unbounded, though stress testing showed it
      stays ~8 MiB).
