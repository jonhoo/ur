//! Clients that connect but never pair: whether and how often the wearer
//! hears about them, and how long they may stay. Hardware-free, for its
//! host tests (`host-tests/unpaired`); [`crate::ble`] keeps a [`Judge`]
//! and a [`Stay`] for each connection, and [`crate::ui`] asks [`Banners`]
//! before showing the "NOT PAIRED" or "PAIRING LOST" banner.
//!
//! Anyone in range can connect to the watch: it advertises, connectably,
//! all the time, and only pairing (with the passkey the watch shows) gets
//! a client anything (see `ble`). One that doesn't pair can't read or
//! write a thing, but it does two kinds of harm just by being there:
//!
//! - **It nags.** Each connection that stays unencrypted gets a banner
//!   (`ble::not_paired`), and a buzz. A phone that thinks it's paired with
//!   the watch (say, one that paired with it under InfiniTime) reconnects
//!   again and again, every few seconds to a minute, all day.
//! - **It takes a slot.** The watch has two connections (`ble::CONNECTIONS`):
//!   the phone and one more, for an updater or a computer fetching the field
//!   log. A stranger holding one keeps those out; two keep the phone out.
//!
//! ## Whether to tell
//!
//! A link that isn't encrypted 10 s in ([`SECURITY_GRACE_MS`]) may just be
//! dying: at the edge of range, a phone can connect and get nothing more
//! through before the link times out. The watch's own phone did that, as
//! its wearer walked away from it, and got PAIRING LOST for it:
//!
//! ```text
//! 23371.311 ble: connected
//! 23381.311 ble: link not encrypted: PairingLost      (10 s, nothing heard)
//! 23381.344 ble: disconnected: Connection Timeout
//! 23384.713 ble: connected                            (and encrypted at once)
//! ```
//!
//! So [`Judge`] tells only of a link that's shown it's alive, and stayed
//! up long enough to prove it wasn't dying:
//!
//! - **It heard the client** ([`Heard`]): a stranger may show it by
//!   anything at all, even answering our request for longer packets; a
//!   bonded phone only by its host talking (an ATT request, or SMP), as a
//!   phone that still had its bond would have encrypted by then. A link
//!   that's silent stays unjudged, and is cut off by [`Stay`] all the same.
//! - **The link outlived its supervision timeout** past the grace, and
//!   past hearing the client: the controller ends a link that hears
//!   nothing for that long, so one still up then was alive after the
//!   grace. It also gives a phone whose link stumbled the time to encrypt.
//! - Or **the client said so itself**: a pairing failed, or used no
//!   passkey ([`Judge::failed`]). The client sent that just now, so it's
//!   told at once, before it may leave.
//!
//! The banner comes ~16 s after connecting rather than 10, and never after
//! the cut-off: one judged later than that is judged at the cut-off.
//!
//! A bonded phone that never talks gets no banner. A phone that forgot its
//! side of the bond (the real PAIRING LOST, FAQ C02) talks: it sends a
//! pairing (which shows the passkey, and is told if it fails), or its app
//! goes ahead unencrypted (Gadgetbridge discovers services and subscribes,
//! and is refused).
//!
//! ## The banner
//!
//! [`Banners`] lets each kind of banner (NOT PAIRED, PAIRING LOST) show at
//! most once every [`BANNER_EVERY_MS`], however often clients reconnect.
//! Each kind on its own: a stranger's NOT PAIRED mustn't hide the wearer's
//! own phone saying PAIRING LOST. The Info screen's phone row still says
//! what's connected right now (`ble::UNPAIRED`), banner or not.
//!
//! ## Staying
//!
//! [`Stay`] says when to disconnect a client whose link isn't encrypted with
//! a passkey bond ("authenticated"), which is all such a link could be for.
//! The phone's own first pairing mustn't be cut short, so the deadline
//! follows how pairing works here:
//!
//! - The watch asks each client to secure the link as it connects (an SMP
//!   Security Request). A bonded phone encrypts within a second. A new one
//!   answers with a pairing, and the watch shows the passkey within a
//!   second or two of that.
//! - That request starts the Security Manager's 30 s timer (the core spec's
//!   SMP timeout, as trouble-host keeps it): a client that hasn't answered
//!   by then can't pair on this connection at all, as trouble-host ignores
//!   its pairing messages from then on. (Android may first ask its user to
//!   consent to the pairing; one who takes longer than that has to try
//!   again, cut-off or not.)
//! - So [`GRACE_MS`] is those 30 s, and time for a pairing that started at
//!   the last moment to reach its passkey.
//! - While the passkey shows, the client gets [`PAIRING_MS`] from when it
//!   appeared: Android gives its user 30 s to type it, and so does the SMP
//!   timer; the rest of the pairing takes a few seconds. Past that, the
//!   pairing is dead: the SMP timer ended it without a word to the
//!   connection (trouble-host sends no failure for a timeout), and the
//!   passkey would otherwise stay up until the client left.
//! - A pairing that ends without authenticating the link (it failed, or
//!   used no passkey) starts a fresh [`GRACE_MS`]: the client may try
//!   again on the same link (once `ble::pairing_backoff` allows).
//!
//! Updates and field log fetches can't be cut off: their services, like the
//! UART, take only authenticated links, so any connection doing either is
//! authenticated, and never has a deadline. Nor is there a bootloader to
//! hand over to: an update arrives over this same connection (see
//! `crate::dfu`), and MCUboot, which installs it after the reset, has no
//! Bluetooth. A paired phone or computer reconnecting after an update
//! encrypts with its bond at once.
//!
//! A cut-off client can come straight back (the slot advertises again), so
//! a persistent stranger still holds a slot most of the time; but each
//! cut-off frees it for a moment, for the phone to take, rather than never.
//!
//! TODO: refuse an address cut off moments ago for a while, if strangers
//! turn out to crowd the phone out in practice. (Most clients can't be
//! told apart for long: their private addresses change every ~15 min, and
//! only a bond resolves them; and a refusal would also catch the wearer's
//! own phone whose user was slow to consent.)

use core::num::NonZeroU32;

/// How long a new connection has to become encrypted before the wearer may
/// hear it isn't (see [`Judge`]). A bonded phone encrypts within a second
/// of our Security Request; a new one shows the passkey by then.
pub const SECURITY_GRACE_MS: u64 = 10_000;

/// How long past its supervision timeout a link must stay up to count as
/// alive (see [`Judge`]): the controller's word that it timed out takes a
/// moment to reach `ble`, and the seconds kept here are rounded.
pub const HOLD_MARGIN_MS: u64 = 1000;

/// What the client was heard sending (see [`Judge::heard`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heard {
    /// A link-layer procedure that took its answer: our request for longer
    /// packets, new connection parameters, a PHY change.
    Link,
    /// A message from its host: an ATT request (refused or not), or SMP (a
    /// pairing, or encryption with a bond).
    Host,
}

/// Where a connection stands on telling the wearer it isn't paired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Told {
    /// Not yet: still to be judged.
    No,
    /// Nothing to tell: the link is authenticated, or a pairing was under
    /// way when it was judged (its failure tells instead).
    Settled,
    /// Told, once and for all for this connection.
    Yes,
}

/// Whether to tell the wearer a connection isn't paired, and when (see
/// the [module docs](self)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Judge {
    /// The second (since boot) the connection began, rounded down.
    connected: u32,
    /// Whether the client is one we have a bond with (as the host knows
    /// from its address at once): it's told as PAIRING LOST, and only its
    /// host talking counts as hearing it.
    bonded: bool,
    /// The second by which the client was first heard in a way that
    /// counts, rounded up (so never 0).
    heard: Option<NonZeroU32>,
    told: Told,
}

impl Judge {
    /// A connection that begins at `now_ms`, with a client we have a bond
    /// with or not.
    pub fn new(now_ms: u64, bonded: bool) -> Self {
        Self {
            connected: seconds(now_ms),
            bonded,
            heard: None,
            told: Told::No,
        }
    }

    /// Whether the client is one we have a bond with: PAIRING LOST rather
    /// than NOT PAIRED.
    pub fn bonded(&self) -> bool {
        self.bonded
    }

    /// Whether the wearer has been told about this connection.
    pub fn told(&self) -> bool {
        self.told == Told::Yes
    }

    /// The client sent `what` at `now_ms`.
    pub fn heard(&mut self, what: Heard, now_ms: u64) {
        // A phone that still has its bond answers the link layer like any
        // other, then encrypts; so for one we have a bond with, only its
        // host talking (unencrypted) says it may have lost its side.
        if self.bonded && what == Heard::Link {
            return;
        }
        if self.heard.is_none() {
            self.heard = NonZeroU32::new(seconds(now_ms).saturating_add(1));
        }
    }

    /// The link is authenticated: nothing to tell, unless a pairing on it
    /// fails later ([`Self::failed`]).
    pub fn settled(&mut self) {
        if self.told == Told::No {
            self.told = Told::Settled;
        }
    }

    /// When to [`Self::check`] (in milliseconds since boot): once the grace
    /// is over and the client was heard, and the link has outlived its
    /// `supervision_timeout_ms` since; at the cut-off (`cut_off_at`, from
    /// [`Stay::cut_off_at`]) at the latest. `None` if it's judged already,
    /// or the client hasn't been heard.
    pub fn due_at(&self, supervision_timeout_ms: u64, cut_off_at: Option<u64>) -> Option<u64> {
        if self.told != Told::No {
            return None;
        }
        let heard = u64::from(self.heard?.get()) * 1000;
        // Rounded up, as `connected` was rounded down: never early.
        let grace_over = (u64::from(self.connected) + 1) * 1000 + SECURITY_GRACE_MS;
        let at = grace_over.max(heard) + supervision_timeout_ms + HOLD_MARGIN_MS;
        Some(cut_off_at.map_or(at, |cut_off| at.min(cut_off)))
    }

    /// Whether to tell the wearer now, at `now_ms`, given the link's
    /// supervision timeout and cut-off (as for [`Self::due_at`]), whether
    /// it's `authenticated`, and whether a `pairing` is under way (showing
    /// its passkey). Says so once only.
    pub fn check(
        &mut self,
        now_ms: u64,
        supervision_timeout_ms: u64,
        cut_off_at: Option<u64>,
        authenticated: bool,
        pairing: bool,
    ) -> bool {
        let due = self
            .due_at(supervision_timeout_ms, cut_off_at)
            .is_some_and(|at| at <= now_ms);
        if !due {
            return false;
        }
        // A pairing under way gets as long as it needs: its failure tells
        // the wearer instead. Encrypted without the passkey ("just works",
        // from a client that can't show or type one) is no better than not
        // at all: our services need `authenticated`.
        if authenticated || pairing {
            self.told = Told::Settled;
            return false;
        }
        self.told = Told::Yes;
        true
    }

    /// A pairing on the link failed, or ended without the passkey: whether
    /// to tell the wearer, at once, given whether the link is
    /// `authenticated` all the same (a phone pairing afresh over a link
    /// encrypted with its bond). Once only, like [`Self::check`].
    pub fn failed(&mut self, authenticated: bool) -> bool {
        if self.told == Told::Yes || authenticated {
            return false;
        }
        self.told = Told::Yes;
        true
    }
}

/// How long between two banners of the same kind. With [`Stay`], a client
/// that won't pair stays [`GRACE_MS`] and comes back moments later: a
/// banner each time would be a buzz a minute, all day. Five minutes keeps
/// it to a dozen an hour at worst, while the wearer setting up their own
/// phone still sees it again within the time they'd plausibly be at it.
///
/// TODO: back off further (say to an hour) while banners keep coming, if
/// a persistent stranger turns out to be common.
pub const BANNER_EVERY_MS: u64 = 5 * 60 * 1000;

/// How many kinds of banner [`Banners`] keeps apart: one per
/// `phone::Unpaired`.
pub const KINDS: usize = 2;

/// When each kind of banner may show next (see the [module docs](self)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Banners {
    /// Per kind, the second (since boot) from which it may show again: 0
    /// for any time. Seconds in a `u32` (136 years) rather than
    /// milliseconds in a `u64`, for half the RAM.
    next: [u32; KINDS],
}

impl Banners {
    /// Whether the banner of kind `kind` (`phone::Unpaired as usize`) may
    /// show at `now_ms`; if so, the next one waits [`BANNER_EVERY_MS`].
    pub fn due(&mut self, kind: usize, now_ms: u64) -> bool {
        let Some(next) = self.next.get_mut(kind) else {
            debug_assert!(false, "no such kind of banner: {kind}");
            return true;
        };
        let now = seconds(now_ms);
        if now < *next {
            return false;
        }
        // Rounded up, as `now` was rounded down: never early.
        *next = seconds((now_ms + BANNER_EVERY_MS).next_multiple_of(1000));
        true
    }
}

/// How long a client that isn't pairing may stay with a link that isn't
/// authenticated: the SMP timeout's 30 s for it to start a pairing, and
/// 10 s for one started then to show its passkey.
pub const GRACE_MS: u64 = 40_000;

/// How long a pairing may show its passkey before the client is cut off:
/// twice the 30 s that Android, and the SMP timeout, give for typing it.
pub const PAIRING_MS: u64 = 60_000;

/// How long one connection may stay unauthenticated (see the
/// [module docs](self)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stay {
    /// The second (since boot) the connection began, or the last pairing
    /// on it showed its passkey or ended, whichever is latest. Seconds, for
    /// RAM's sake: this lives in each connection slot's future.
    since: u32,
    /// Whether a pairing is under way: it has shown its passkey and not
    /// yet ended.
    pairing: bool,
    /// Whether [`Self::check`] has cut the client off: once is enough,
    /// while the disconnection takes its time.
    cut_off: bool,
}

impl Stay {
    /// A connection that begins at `now_ms`.
    pub fn new(now_ms: u64) -> Self {
        Self {
            since: seconds(now_ms),
            pairing: false,
            cut_off: false,
        }
    }

    /// Whether a pairing is under way (see [`Self::pairing_started`]).
    pub fn pairing(&self) -> bool {
        self.pairing
    }

    /// A pairing showed its passkey at `now_ms`.
    pub fn pairing_started(&mut self, now_ms: u64) {
        self.since = seconds(now_ms);
        self.pairing = true;
    }

    /// The pairing under way ended at `now_ms`, however it went.
    pub fn pairing_ended(&mut self, now_ms: u64) {
        self.since = seconds(now_ms);
        self.pairing = false;
    }

    /// When to [`Self::check`] (in milliseconds since boot) whether to
    /// disconnect the client: `None` if its link is `authenticated`, or
    /// it's been cut off already.
    pub fn cut_off_at(&self, authenticated: bool) -> Option<u64> {
        if authenticated || self.cut_off {
            return None;
        }
        let allowed = if self.pairing { PAIRING_MS } else { GRACE_MS };
        // Rounded up to the second, as `since` was rounded down: never
        // early.
        Some((u64::from(self.since) + 1) * 1000 + allowed)
    }

    /// Whether to disconnect the client at `now_ms`, given whether its link
    /// is `authenticated`. Says so once only.
    pub fn check(&mut self, authenticated: bool, now_ms: u64) -> bool {
        let due = self
            .cut_off_at(authenticated)
            .is_some_and(|at| at <= now_ms);
        self.cut_off |= due;
        due
    }
}

/// `ms` in whole seconds, rounded down.
fn seconds(ms: u64) -> u32 {
    u32::try_from(ms / 1000).unwrap_or(u32::MAX)
}
