//! Host tests for the firmware's `unpaired` module: whether and how often
//! the wearer hears about clients that don't pair, and how long such a
//! client may stay connected. `cargo test` runs them.

#[path = "../../../src/unpaired.rs"]
#[allow(dead_code)]
mod unpaired;

fn main() {
    println!("run `cargo test`");
}

#[cfg(test)]
mod tests {
    use super::unpaired::*;

    const SECOND: u64 = 1000;
    const MINUTE: u64 = 60 * SECOND;
    /// `phone::Unpaired`'s kinds, as `ui` passes them.
    const NOT_PAIRED: usize = 0;
    const PAIRING_LOST: usize = 1;

    /// Android's supervision timeout, as the field logs show it.
    const TIMEOUT: u64 = 5 * SECOND;

    /// What `ble::serve` would do with a connection that begins at
    /// `connected` and ends (the controller's supervision timeout, or the
    /// client leaving) at `ends`, given what it hears from the client and
    /// when (`heard`), and whether the link is ever authenticated: when it
    /// tells the wearer, if it does. Steps from deadline to deadline as the
    /// event loop does, the cut-off included.
    fn told_at(
        connected: u64,
        bonded: bool,
        heard: &[(Heard, u64)],
        ends: u64,
        timeout: u64,
    ) -> Option<u64> {
        let mut judge = Judge::new(connected, bonded);
        let mut stay = Stay::new(connected);
        let mut events = heard.iter().copied().peekable();
        let mut now = connected;
        loop {
            let cut_off = stay.cut_off_at(false);
            let deadline = [judge.due_at(timeout, cut_off), cut_off]
                .into_iter()
                .flatten()
                .min();
            let next_event = events.peek().map(|&(_, at)| at);
            // The disconnection comes first, as `select4` polls the
            // connection's events first.
            let next = [deadline, next_event, Some(ends)]
                .into_iter()
                .flatten()
                .min()
                .expect("it ends");
            assert!(next >= now, "time went backwards");
            now = next;
            if now >= ends {
                return None;
            }
            if next_event == Some(now) {
                let (what, at) = events.next().expect("peeked");
                judge.heard(what, at);
                continue;
            }
            if judge.check(now, timeout, stay.cut_off_at(false), false, false) {
                return Some(now);
            }
            if stay.check(false, now) {
                return None;
            }
        }
    }

    // ==========================================================================
    // Whether to tell
    // ==========================================================================

    /// The field log that started this (sealed watch, 0.11.0): the bonded
    /// phone, walking out of range, reconnected and got nothing through,
    /// and the link timed out 10.03 s later. It got PAIRING LOST at 10 s.
    #[test]
    fn a_bonded_phone_whose_link_dies_is_not_told() {
        let connected = 23_371_311;
        let ends = 23_381_345;
        assert_eq!(told_at(connected, true, &[], ends, TIMEOUT), None);
        // Nor if the link layer got something through first: the phone
        // answering our request for longer packets, say.
        let link = [(Heard::Link, connected + 94)];
        assert_eq!(told_at(connected, true, &link, ends, TIMEOUT), None);
        // Nor even if its host did (Android's MTU exchange), as the link
        // died within its supervision timeout of that.
        let host = [(Heard::Host, connected + 200)];
        assert_eq!(told_at(connected, true, &host, ends, TIMEOUT), None);
    }

    /// The judgement never comes due while the link is still proving it
    /// isn't dying: a link whose last sign of life came within the grace
    /// times out (one supervision timeout later) untold, however the
    /// times fall. (One heard after the grace was alive after it, and is
    /// judged as alive.)
    #[test]
    fn no_link_that_dies_within_the_grace_is_told() {
        for bonded in [false, true] {
            for timeout in [2 * SECOND, TIMEOUT, 6 * SECOND, 20 * SECOND] {
                for connected in [0, 999, 1000, 23_371_311] {
                    for last in (0..=20).map(|s| connected + s * SECOND / 2 + 7) {
                        let heard = [(Heard::Host, connected + 1), (Heard::Host, last)];
                        let ends = last + timeout;
                        assert_eq!(
                            told_at(connected, bonded, &heard, ends, timeout),
                            None,
                            "bonded {bonded}, timeout {timeout}, last heard {last}"
                        );
                    }
                }
            }
        }
    }

    /// A stranger with a healthy link is told: its link layer answered at
    /// once, and the link stayed. After the grace and one supervision
    /// timeout, and a little more.
    #[test]
    fn a_stranger_that_stays_is_told() {
        let connected = 12 * MINUTE + 345;
        let heard = [(Heard::Link, connected + 80)];
        let at = told_at(connected, false, &heard, connected + MINUTE, TIMEOUT).expect("told");
        assert!(
            at >= connected + SECURITY_GRACE_MS + TIMEOUT,
            "{at} is early"
        );
        assert!(
            at <= connected + SECURITY_GRACE_MS + TIMEOUT + HOLD_MARGIN_MS + SECOND,
            "{at} is late"
        );
    }

    /// The real PAIRING LOST (FAQ C02): the phone forgot its bond, and
    /// Gadgetbridge goes ahead unencrypted, discovering services and
    /// being refused its subscription. Told, as before, if a little later.
    #[test]
    fn a_bonded_phone_talking_unencrypted_is_told() {
        let connected = 3 * MINUTE;
        let heard = [
            (Heard::Link, connected + 60),
            (Heard::Host, connected + 150),
            (Heard::Host, connected + 900),
        ];
        let at = told_at(connected, true, &heard, connected + MINUTE, TIMEOUT).expect("told");
        assert!(at < connected + 20 * SECOND, "{at} is late");
    }

    /// A bonded phone whose host never says a thing is never told (it's
    /// most likely stuck, not without its bond), only cut off.
    #[test]
    fn a_silent_bonded_phone_is_only_cut_off() {
        let connected = 3 * MINUTE;
        let heard = [
            (Heard::Link, connected + 60),
            (Heard::Link, connected + 900),
        ];
        assert_eq!(
            told_at(connected, true, &heard, connected + MINUTE, TIMEOUT),
            None
        );
        let mut judge = Judge::new(connected, true);
        judge.heard(Heard::Link, connected + 60);
        assert_eq!(judge.due_at(TIMEOUT, None), None);
    }

    /// A client heard only after the grace (its link stumbled at first) is
    /// judged a supervision timeout after that, not at once: a phone with
    /// its bond would be encrypting by then.
    #[test]
    fn a_client_heard_late_is_judged_later() {
        let connected = 0;
        let mut judge = Judge::new(connected, true);
        let late = 25 * SECOND + 400;
        judge.heard(Heard::Host, late);
        let at = judge.due_at(TIMEOUT, None).expect("a deadline");
        assert!(at >= late + TIMEOUT, "{at} is early");
        assert!(
            at <= late + TIMEOUT + HOLD_MARGIN_MS + SECOND,
            "{at} is late"
        );
        // Only the first counts.
        judge.heard(Heard::Host, late + 3 * SECOND);
        assert_eq!(judge.due_at(TIMEOUT, None), Some(at));
    }

    /// A supervision timeout so long the judgement would come after the
    /// cut-off is judged at the cut-off: still up then, the client is told
    /// (and cut off).
    #[test]
    fn the_cut_off_judges_at_the_latest() {
        let connected = 0;
        let stay = Stay::new(connected);
        let cut_off = stay.cut_off_at(false);
        let mut judge = Judge::new(connected, false);
        judge.heard(Heard::Link, 100);
        assert_eq!(judge.due_at(32 * SECOND, cut_off), cut_off);
        let heard = [(Heard::Link, 100)];
        assert_eq!(
            told_at(connected, false, &heard, MINUTE, 32 * SECOND),
            cut_off
        );
    }

    /// An authenticated link, or a pairing under way when the judgement
    /// comes, settles it for good; a pairing that then fails tells.
    #[test]
    fn a_secured_link_is_not_told() {
        let mut judge = Judge::new(0, false);
        judge.heard(Heard::Host, 100);
        let at = judge.due_at(TIMEOUT, None).expect("a deadline");
        assert!(!judge.check(at, TIMEOUT, None, true, false));
        assert_eq!(judge.due_at(TIMEOUT, None), None);
        assert!(!judge.check(at + MINUTE, TIMEOUT, None, false, false));

        let mut judge = Judge::new(0, false);
        judge.heard(Heard::Host, 100);
        judge.settled();
        assert_eq!(judge.due_at(TIMEOUT, None), None);

        let mut judge = Judge::new(0, false);
        judge.heard(Heard::Host, 100);
        let at = judge.due_at(TIMEOUT, None).expect("a deadline");
        assert!(!judge.check(at, TIMEOUT, None, false, true));
        assert!(!judge.told());
        assert!(judge.failed(false));
        assert!(judge.told());
    }

    /// `check` tells at the deadline, not before, and once only.
    #[test]
    fn the_judgement_comes_once() {
        let mut judge = Judge::new(0, true);
        assert!(judge.bonded());
        judge.heard(Heard::Host, 100);
        let at = judge.due_at(TIMEOUT, None).expect("a deadline");
        assert!(!judge.check(at - 1, TIMEOUT, None, false, false));
        assert!(judge.check(at, TIMEOUT, None, false, false));
        assert!(judge.told());
        assert_eq!(judge.due_at(TIMEOUT, None), None);
        assert!(!judge.check(at, TIMEOUT, None, false, false));
        assert!(!judge.failed(false));
    }

    /// A pairing that fails (or uses no passkey) tells at once, whether or
    /// not the client was heard otherwise: the failure is its word. Not
    /// over a link authenticated all the same, and once only.
    #[test]
    fn a_failed_pairing_tells_at_once() {
        let mut judge = Judge::new(0, true);
        assert!(!judge.failed(true));
        assert!(judge.failed(false));
        assert!(!judge.failed(false));
        let mut judge = Judge::new(0, false);
        judge.settled();
        assert!(judge.failed(false));
    }

    /// Never early, whatever the milliseconds: the grace and the hold are
    /// whole, from the moment of connecting and of hearing.
    #[test]
    fn the_judgement_is_never_early() {
        for connected in [0, 1, 999, 1000, 1001, 59_999, 123_456_789] {
            for after in [0, 1, 999, 1000, 9_999, 10_001, 30_000] {
                let heard = connected + after;
                let mut judge = Judge::new(connected, false);
                judge.heard(Heard::Host, heard);
                let at = judge.due_at(TIMEOUT, None).expect("a deadline");
                assert!(at >= connected + SECURITY_GRACE_MS + TIMEOUT + HOLD_MARGIN_MS);
                assert!(at >= heard + TIMEOUT + HOLD_MARGIN_MS);
            }
        }
    }

    // ==========================================================================
    // The banner
    // ==========================================================================

    #[test]
    fn the_first_banner_shows_even_at_boot() {
        assert!(Banners::default().due(NOT_PAIRED, 0));
        assert!(Banners::default().due(PAIRING_LOST, 1));
    }

    /// A stranger reconnecting every ~45 s (as [`Stay`] cuts it off) for
    /// an hour gets a banner every five minutes, and never sooner.
    #[test]
    fn a_reconnecting_client_is_shown_every_five_minutes() {
        let mut banners = Banners::default();
        let start = 3 * 60 * MINUTE + 123;
        let mut shown = Vec::new();
        let mut now = start;
        while now < start + 60 * MINUTE {
            if banners.due(NOT_PAIRED, now) {
                shown.push(now);
            }
            now += 45 * SECOND + 300;
        }
        assert_eq!(shown.len(), 12, "{shown:?}");
        for pair in shown.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(
                (BANNER_EVERY_MS..BANNER_EVERY_MS + 46 * SECOND).contains(&gap),
                "{gap} ms between banners"
            );
        }
    }

    /// Never early, whatever the milliseconds: the seconds kept are
    /// rounded to wait at least the whole interval.
    #[test]
    fn the_interval_is_never_cut_short() {
        for first in [0, 1, 999, 1000, 1001, 59_999, 123_456_789] {
            let mut banners = Banners::default();
            assert!(banners.due(NOT_PAIRED, first));
            assert!(!banners.due(NOT_PAIRED, first + BANNER_EVERY_MS - 1));
            assert!(banners.due(NOT_PAIRED, first + BANNER_EVERY_MS + 999));
        }
    }

    /// The wearer's own phone saying PAIRING LOST isn't hidden by a
    /// stranger's NOT PAIRED a moment before, nor the other way round;
    /// and a refused banner doesn't push its own next one further away.
    #[test]
    fn each_kind_keeps_its_own_time() {
        let mut banners = Banners::default();
        assert!(banners.due(NOT_PAIRED, MINUTE));
        assert!(banners.due(PAIRING_LOST, MINUTE + SECOND));
        assert!(!banners.due(NOT_PAIRED, 2 * MINUTE));
        assert!(!banners.due(PAIRING_LOST, 2 * MINUTE));
        assert!(!banners.due(NOT_PAIRED, 5 * MINUTE));
        assert!(banners.due(NOT_PAIRED, 6 * MINUTE));
        assert!(!banners.due(PAIRING_LOST, 6 * MINUTE));
        assert!(banners.due(PAIRING_LOST, 6 * MINUTE + 2 * SECOND));
    }

    // ==========================================================================
    // Staying
    // ==========================================================================

    #[test]
    fn an_authenticated_link_stays() {
        let mut stay = Stay::new(5 * SECOND);
        assert_eq!(stay.cut_off_at(true), None);
        stay.pairing_started(6 * SECOND);
        assert_eq!(stay.cut_off_at(true), None);
        stay.pairing_ended(20 * SECOND);
        assert_eq!(stay.cut_off_at(true), None);
    }

    /// One that never pairs goes once the SMP timeout (30 s from our
    /// Security Request, sent as it connects) has passed, and a pairing
    /// started just before it could have shown its passkey.
    #[test]
    fn a_client_that_never_pairs_goes_after_the_grace() {
        let connected = 12 * MINUTE + 345;
        let stay = Stay::new(connected);
        let at = stay.cut_off_at(false).expect("a deadline");
        assert!(at >= connected + GRACE_MS, "{at} is early");
        assert!(at < connected + GRACE_MS + SECOND, "{at} is late");
    }

    /// The wearer pairing their phone: the passkey shows a second after
    /// connecting, and they take Android's whole 30 s to type it. Never
    /// cut off along the way; the deadline moves to the pairing's own.
    #[test]
    fn a_slow_first_pairing_is_not_cut_short() {
        let connected = 7 * SECOND + 500;
        let mut stay = Stay::new(connected);
        let shown = connected + SECOND;
        stay.pairing_started(shown);
        assert!(stay.pairing());
        let at = stay.cut_off_at(false).expect("a deadline while pairing");
        assert!(at >= shown + PAIRING_MS);
        // Typing ends Android's 30 s later, the rest takes seconds.
        assert!(at > shown + 30 * SECOND + 10 * SECOND);
        // Done: authenticated, and staying.
        stay.pairing_ended(shown + 35 * SECOND);
        assert!(!stay.pairing());
        assert_eq!(stay.cut_off_at(true), None);
    }

    /// A pairing that started at the last moment (just before the SMP
    /// timeout) still reaches its passkey before the grace runs out.
    #[test]
    fn a_pairing_started_late_reaches_its_passkey() {
        let connected = 100 * SECOND + 999;
        let mut stay = Stay::new(connected);
        let passkey = connected + 30 * SECOND + 2 * SECOND;
        assert!(stay.cut_off_at(false).expect("a deadline") > passkey);
        stay.pairing_started(passkey);
        assert!(stay.cut_off_at(false).expect("a deadline") >= passkey + PAIRING_MS);
    }

    /// A pairing that failed (a mistyped passkey) leaves the client a
    /// fresh grace to try again on the same link, then it goes.
    #[test]
    fn a_failed_pairing_gets_a_fresh_grace() {
        let mut stay = Stay::new(0);
        stay.pairing_started(2 * SECOND);
        let failed = 20 * SECOND + 1;
        stay.pairing_ended(failed);
        let at = stay.cut_off_at(false).expect("a deadline");
        assert!((failed + GRACE_MS..failed + GRACE_MS + SECOND).contains(&at));
    }

    /// A pairing whose passkey nobody types (or that the SMP timer ended
    /// silently) can't hold the slot, nor keep the passkey up, forever.
    #[test]
    fn a_stalled_pairing_is_cut_off() {
        let mut stay = Stay::new(0);
        stay.pairing_started(3 * SECOND);
        let at = stay.cut_off_at(false).expect("a deadline");
        assert!(at <= 3 * SECOND + PAIRING_MS + SECOND);
    }

    /// `check` says to disconnect at the deadline, not before, and once
    /// only: the link lingers while the disconnection goes through, and
    /// mustn't be disconnected (and logged) again at every turn. A failure
    /// reported meanwhile doesn't rearm it, and still counts as a pairing.
    #[test]
    fn the_cut_off_comes_once() {
        let mut stay = Stay::new(0);
        stay.pairing_started(SECOND);
        let at = stay.cut_off_at(false).expect("a deadline");
        assert!(!stay.check(false, at - 1));
        assert!(!stay.check(true, at));
        assert!(stay.check(false, at));
        assert!(!stay.check(false, at));
        assert!(stay.pairing());
        assert_eq!(stay.cut_off_at(false), None);
        stay.pairing_ended(at + 100);
        assert_eq!(stay.cut_off_at(false), None);
        assert!(!stay.check(false, at + 10 * MINUTE));
    }

    /// Decades after boot, nothing overflows.
    #[test]
    fn far_from_boot() {
        let late = u64::from(u32::MAX / 2) * 1000;
        let stay = Stay::new(late);
        assert!(stay.cut_off_at(false).expect("a deadline") > late);
        let mut banners = Banners::default();
        assert!(banners.due(NOT_PAIRED, late));
        assert!(!banners.due(NOT_PAIRED, late + MINUTE));
        let mut judge = Judge::new(late, false);
        judge.heard(Heard::Link, late + 1);
        assert!(judge.due_at(TIMEOUT, None).expect("a deadline") > late);
        let mut judge = Judge::new(u64::from(u32::MAX) * 1000 + 999, false);
        judge.heard(Heard::Link, u64::MAX / 2);
        assert!(judge.due_at(TIMEOUT, None).is_some());
    }
}
