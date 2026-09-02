//! The second outbound path: messages the app owes a human, delivered from a table.
//!
//! The driver loop in [`super`] writes to the user in exactly one place — the reply to
//! the message it is currently answering — and then the iteration ends. Everything that
//! finishes later had nowhere to go. This worker is that somewhere.
//!
//! It is a table rather than a channel for one reason: the restart that loses an
//! in-memory queue is precisely the one that happens during a long-running task, which
//! is the case the whole feature exists for. A queued message survives the process, and
//! it goes to the chat that asked — read off the row — not to wherever anybody last
//! spoke, which by then is very likely somewhere else.
//!
//! Delivery is at-least-once and deliberately so. A message sent twice is a small
//! annoyance; a message sent zero times is the bug.

use crate::storage::database::OutboxMessage;
use crate::storage::Db;

/// How often the queue is looked at. Fast enough to feel immediate on a phone, and the
/// work when the queue is empty is one indexed query.
const TICK: std::time::Duration = std::time::Duration::from_secs(5);

/// How long a claimed message may sit before it is assumed the worker holding it died.
///
/// Longer than any single send: WhatsApp goes through a sidecar over stdin, and taking a
/// message back while it is genuinely mid-flight is how one notification becomes two.
const CLAIM_TIMEOUT_MINS: i64 = 5;

/// Waits between attempts: seconds, half a minute, two minutes, ten, half an hour, two
/// hours. The shape matters more than the numbers — a transport that is down for a
/// moment recovers in seconds, and one that is down for the evening should not be
/// hammered all evening.
const BACKOFF_SECS: [i64; 6] = [5, 30, 120, 600, 1800, 7200];

/// After this many failures a message is dead. It gets one last try somewhere else
/// first: the original chat may be gone — a deleted group, a revoked token — while the
/// person is still reachable where they last spoke.
const MAX_ATTEMPTS: i64 = 6;

/// Start the outbox worker. One per process, alongside the transport drivers.
pub fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        loop {
            tokio::time::sleep(TICK).await;
            let db = app.state::<Db>().inner().clone();
            tick(&db, send_over_transport).await;
        }
    });
}

/// What actually puts bytes on a transport. Injected so the retry, claim and give-up
/// rules can be tested without a network or a sidecar — those rules are the part that
/// decides whether a person hears anything.
pub type Sending = std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>;
pub type Sender = fn(crate::ai::remote::Route, String) -> Sending;

fn send_over_transport(route: crate::ai::remote::Route, body: String) -> Sending {
    Box::pin(async move { crate::ai::remote::send_to(&route, &body).await })
}

/// One pass: recover anything abandoned mid-send, then deliver at most one message.
///
/// One at a time on purpose. The queue is short by construction, and a burst of
/// notifications arriving together on a phone reads worse than the same ones spaced five
/// seconds apart.
pub async fn tick(db: &Db, send: Sender) {
    let cutoff = (chrono::Utc::now() - chrono::Duration::minutes(CLAIM_TIMEOUT_MINS))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    match db.requeue_stalled_outbox(&cutoff) {
        Ok(n) if n > 0 => crate::diag(&format!("outbox: returned {n} stalled message(s) to the queue")),
        _ => {}
    }
    let row = match db.claim_outbox() {
        Ok(Some(row)) => row,
        Ok(None) => return,
        Err(e) => {
            crate::diag(&format!("outbox: could not read the queue: {e}"));
            return;
        }
    };
    deliver(db, &row, send).await;
}

/// Deliver one claimed message, and record what happened to it either way.
pub async fn deliver(db: &Db, row: &OutboxMessage, send: Sender) {
    // The route comes off the row. This is the whole fix: an answer goes back to the
    // chat that asked the question, whatever has been said anywhere since.
    let route = match crate::ai::remote::Kind::parse(&row.transport) {
        Some(kind) if !row.chat_id.trim().is_empty() => crate::ai::remote::Route {
            kind,
            chat_id: row.chat_id.clone(),
        },
        // Unsendable as written, and no number of retries changes that.
        _ => {
            let _ = db.fail_outbox(&row.id, "no route on the row", None);
            close(db, row, "abandoned");
            return;
        }
    };

    match send(route, row.body.clone()).await {
        Ok(()) => {
            let _ = db.mark_outbox_sent(&row.id);
            close(db, row, "answered");
            return;
        }
        Err(e) => {
            let attempts = row.attempts + 1;
            if attempts < MAX_ATTEMPTS {
                let wait = BACKOFF_SECS
                    .get(row.attempts.max(0) as usize)
                    .copied()
                    .unwrap_or(*BACKOFF_SECS.last().unwrap());
                let retry_at = (chrono::Utc::now() + chrono::Duration::seconds(wait))
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string();
                let _ = db.fail_outbox(&row.id, &e, Some(&retry_at));
                crate::diag(&format!(
                    "outbox: {} failed ({e}); retrying in {wait}s (attempt {attempts})",
                    row.id
                ));
                return;
            }

            // Out of attempts on the chat that asked. One last try wherever the user
            // last spoke — a deleted group or a revoked token does not mean the person
            // is unreachable.
            crate::diag(&format!("outbox: {} failed {attempts} times: {e}", row.id));
            let fallback = crate::ai::remote::last_route(db)
                .filter(|r| r.kind.as_str() != row.transport || r.chat_id != row.chat_id);
            if let Some(route) = fallback {
                if send(route, row.body.clone()).await.is_ok() {
                    let _ = db.mark_outbox_sent(&row.id);
                    close(db, row, "answered");
                    return;
                }
            }
            let _ = db.fail_outbox(&row.id, &e, None);
            close(db, row, "abandoned");
        }
    }
}

/// Settle the ask this message answered, if it answered one.
fn close(db: &Db, row: &OutboxMessage, status: &str) {
    if let Some(request_id) = row.request_id.as_deref() {
        let _ = db.close_remote_request(request_id, status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::remote::{Kind, Route};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    fn db() -> Db {
        Db::open(std::path::Path::new(":memory:")).unwrap()
    }

    fn queue(d: &Db, id: &str, request_id: Option<&str>, transport: &str, chat: &str) {
        d.enqueue_outbox(&OutboxMessage {
            id: id.to_string(),
            request_id: request_id.map(str::to_string),
            goal_id: Some("g1".into()),
            transport: transport.to_string(),
            chat_id: chat.to_string(),
            body: "[xConsole] web-1 is at 91%".into(),
            state: "pending".into(),
            attempts: 0,
            next_attempt_at: None,
            last_error: None,
            dedupe_key: format!("k:{id}"),
            created_at: None,
            sent_at: None,
        })
        .unwrap();
    }

    fn request(d: &Db, id: &str, transport: &str, chat: &str) {
        d.insert_remote_request(&crate::storage::database::RemoteRequest {
            id: id.into(),
            transport: transport.into(),
            chat_id: chat.into(),
            author_id: "40712345678".into(),
            message_id: None,
            persona_id: None,
            ask: "check the disk".into(),
            status: "open".into(),
            created_at: None,
            closed_at: None,
        })
        .unwrap();
    }

    static WENT_TO: Mutex<Vec<String>> = Mutex::new(Vec::new());
    fn deliver_ok(route: Route, _body: String) -> Sending {
        WENT_TO.lock().unwrap().push(route.encode());
        Box::pin(async { Ok(()) })
    }

    #[tokio::test]
    async fn a_queued_message_survives_and_goes_to_the_chat_that_asked() {
        // The bug this replaces answered "wherever we last spoke", which an hour after
        // the question is very likely somewhere else entirely. Nothing about this
        // delivery is in memory: the row is the whole record, which is also why it
        // survives the restart that is most likely to happen during a long task.
        let d = db();
        request(&d, "req-1", "telegram", "tg-asked");
        queue(&d, "o1", Some("req-1"), "telegram", "tg-asked");
        // Somebody has since said something on WhatsApp, so "last route" is wrong.
        crate::ai::remote::remember_route(&d, &Route { kind: Kind::WhatsApp, chat_id: "wa-9".into() });

        WENT_TO.lock().unwrap().clear();
        tick(&d, deliver_ok).await;

        assert_eq!(*WENT_TO.lock().unwrap(), vec!["telegram:tg-asked".to_string()]);
        let row = d.get_outbox("o1").unwrap().unwrap();
        assert_eq!(row.state, "sent");
        assert!(row.sent_at.is_some());
        // And the ask it answered is settled, so the sweep does not later announce it
        // as abandoned.
        assert_eq!(d.get_remote_request("req-1").unwrap().unwrap().status, "answered");
        assert!(d.list_open_remote_requests().unwrap().is_empty());
    }

    fn refuse(_route: Route, _body: String) -> Sending {
        Box::pin(async { Err("the WhatsApp helper is not running".to_string()) })
    }

    #[tokio::test]
    async fn a_transport_failure_is_retried_rather_than_dropped() {
        // The sidecar is not always up, and a message dropped because of that is the
        // silence this whole path exists to remove.
        let d = db();
        request(&d, "req-2", "whatsapp", "wa-1");
        queue(&d, "o1", Some("req-2"), "whatsapp", "wa-1");

        tick(&d, refuse).await;
        let row = d.get_outbox("o1").unwrap().unwrap();
        assert_eq!(row.state, "pending", "still owed");
        assert_eq!(row.attempts, 1);
        assert!(row.next_attempt_at.is_some(), "and scheduled to try again");
        assert_eq!(row.last_error.as_deref(), Some("the WhatsApp helper is not running"));
        // The ask stays open while the answer is still owed.
        assert_eq!(d.get_remote_request("req-2").unwrap().unwrap().status, "open");

        // Backing off, not hammering: it is not due yet, so the next tick leaves it be.
        tick(&d, refuse).await;
        assert_eq!(d.get_outbox("o1").unwrap().unwrap().attempts, 1);
    }

    static TRIES: AtomicUsize = AtomicUsize::new(0);
    fn count_and_refuse(_route: Route, _body: String) -> Sending {
        TRIES.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err("chat not found".to_string()) })
    }

    #[tokio::test]
    async fn six_failures_mark_a_message_dead_after_one_last_try_elsewhere() {
        // Retrying forever against a deleted group is its own kind of broken. Giving up
        // silently is worse, so the last attempt goes wherever the user was last seen.
        let d = db();
        request(&d, "req-3", "telegram", "tg-gone");
        queue(&d, "o1", Some("req-3"), "telegram", "tg-gone");
        crate::ai::remote::remember_route(&d, &Route { kind: Kind::WhatsApp, chat_id: "wa-1".into() });

        TRIES.store(0, Ordering::SeqCst);
        for _ in 0..6 {
            let row = d.get_outbox("o1").unwrap().unwrap();
            deliver(&d, &row, count_and_refuse).await;
        }

        let row = d.get_outbox("o1").unwrap().unwrap();
        assert_eq!(row.attempts, 6);
        assert_eq!(row.state, "dead");
        assert_eq!(TRIES.load(Ordering::SeqCst), 7, "six on the chat that asked, one elsewhere");
        // Said out loud in the data: the ask was never answered.
        assert_eq!(d.get_remote_request("req-3").unwrap().unwrap().status, "abandoned");
    }

    static SENDS: AtomicUsize = AtomicUsize::new(0);
    fn count_ok(_route: Route, _body: String) -> Sending {
        SENDS.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }

    #[tokio::test]
    async fn a_claimed_message_is_not_claimed_again() {
        // Delivery is at-least-once by design, but "at least" must not mean "every five
        // seconds while the first send is still in flight".
        let d = db();
        queue(&d, "o1", None, "telegram", "tg-1");
        let claimed = d.claim_outbox().unwrap().expect("the only row");
        assert_eq!(claimed.id, "o1");
        assert!(d.claim_outbox().unwrap().is_none(), "a claimed row is nobody else's");

        SENDS.store(0, Ordering::SeqCst);
        tick(&d, count_ok).await;
        assert_eq!(SENDS.load(Ordering::SeqCst), 0, "the tick found nothing to do");

        // Unless the process holding it died: after long enough, it is owed again.
        let future = (chrono::Utc::now() + chrono::Duration::minutes(10))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        assert_eq!(d.requeue_stalled_outbox(&future).unwrap(), 1);
        tick(&d, count_ok).await;
        assert_eq!(SENDS.load(Ordering::SeqCst), 1);
        assert_eq!(d.get_outbox("o1").unwrap().unwrap().state, "sent");
    }

    #[tokio::test]
    async fn a_row_with_no_usable_route_is_not_retried_forever() {
        let d = db();
        request(&d, "req-4", "carrier-pigeon", "");
        queue(&d, "o1", Some("req-4"), "carrier-pigeon", "nowhere");
        tick(&d, refuse).await;
        let row = d.get_outbox("o1").unwrap().unwrap();
        assert_eq!(row.state, "dead");
        assert_eq!(d.get_remote_request("req-4").unwrap().unwrap().status, "abandoned");
    }
}
