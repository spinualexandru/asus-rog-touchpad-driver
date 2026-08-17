use super::TouchPosition;

/// Upper bound on simultaneously tracked contacts.
///
/// Slot ids come from the hardware, so they are never used as an index into this
/// table; the cap only stops a pad that reports begins without matching ends from
/// growing it without limit.
const MAX_TRACKED_CONTACTS: usize = 16;

#[derive(Debug, Clone, Copy)]
struct Contact {
    slot: i32,
    position: TouchPosition,
    /// Set for the report in which this contact began and cleared by `end_report`.
    /// Only a contact that is new in that sense may fire a key.
    is_new: bool,
}

/// Tracks which multitouch contacts are down and which of them owns the position.
///
/// Multitouch protocol B reports `ABS_MT_SLOT` only when it changes, so the slot is
/// sticky between reports; `ABS_MT_TRACKING_ID` >= 0 starts a contact in the current
/// slot and -1 ends it. Watching the slot alone is not enough. When the slot holding
/// the position lifts while a second finger stays down, the pad drops back to
/// `BTN_TOOL_FINGER=1` and the driver would otherwise act on the coordinate of the
/// finger that just left — typing whatever key it was resting on.
#[derive(Debug, Default)]
pub struct ContactTracker {
    current_slot: i32,
    /// Contacts currently down, in the order they landed.
    contacts: Vec<Contact>,
    /// Slot whose position the driver follows; `None` once the pad is empty.
    primary: Option<i32>,
    /// Stays false until the pad reports its first tracking id. Single-touch pads
    /// report neither slots nor lifetimes, and must keep the pre-slot behaviour.
    multitouch: bool,
}

impl ContactTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an `ABS_MT_SLOT` value. Sticky: protocol B only sends it on change.
    pub fn set_slot(&mut self, slot: i32) {
        self.current_slot = slot;
    }

    /// Feeds an `ABS_MT_TRACKING_ID` for the current slot: >= 0 begins a contact
    /// there, -1 ends it.
    ///
    /// Returns true when that lift handed position ownership to a finger that is
    /// still down, in which case the caller should adopt `primary_position`.
    pub fn track(&mut self, tracking_id: i32) -> bool {
        self.multitouch = true;

        if tracking_id >= 0 {
            self.begin_contact();
            false
        } else {
            self.end_contact()
        }
    }

    fn begin_contact(&mut self) {
        let slot = self.current_slot;

        if let Some(contact) = self.contact_mut(slot) {
            // A begin for a slot already down means we missed its end; treat the
            // slot as freshly touched rather than leaving a contact that can never
            // be released.
            contact.is_new = true;
        } else {
            if self.contacts.len() >= MAX_TRACKED_CONTACTS {
                return;
            }
            self.contacts.push(Contact {
                slot,
                position: TouchPosition::default(),
                is_new: true,
            });
        }

        if self.primary.is_none() {
            self.primary = Some(slot);
        }
    }

    fn end_contact(&mut self) -> bool {
        let slot = self.current_slot;
        self.contacts.retain(|contact| contact.slot != slot);

        if self.primary != Some(slot) {
            return false;
        }

        // Hand ownership to the oldest finger still down, so the position follows a
        // live contact instead of freezing on the one that just left.
        self.primary = self.contacts.first().map(|contact| contact.slot);
        self.primary.is_some()
    }

    /// True when `ABS_MT_POSITION_*` for the current slot should move the position.
    ///
    /// Without this a second resting finger drags the position to itself.
    pub fn current_slot_owns_position(&self) -> bool {
        match self.primary {
            Some(primary) => primary == self.current_slot,
            // No tracking id seen yet: fall back to the old slot-0 rule so a pad
            // that reports coordinates before lifetimes still works.
            None => self.current_slot == 0,
        }
    }

    /// Records a coordinate for the current slot, primary or not — a secondary
    /// finger's position is what ownership is handed over to when the primary lifts.
    pub fn update_x(&mut self, x: f64) {
        let slot = self.current_slot;
        if let Some(contact) = self.contact_mut(slot) {
            contact.position.x = x;
        }
    }

    /// See `update_x`.
    pub fn update_y(&mut self, y: f64) {
        let slot = self.current_slot;
        if let Some(contact) = self.contact_mut(slot) {
            contact.position.y = y;
        }
    }

    /// Position of the contact the driver follows, or `None` when the pad is empty.
    pub fn primary_position(&self) -> Option<TouchPosition> {
        let primary = self.primary?;
        self.contact(primary).map(|contact| contact.position)
    }

    /// Slot the driver currently follows, if any. Exposed for logging and tests.
    pub fn primary_slot(&self) -> Option<i32> {
        self.primary
    }

    /// True when the followed contact began in the report being handled — the only
    /// case in which a finger-down is a real tap.
    ///
    /// Dropping from two fingers back to one also raises `BTN_TOOL_FINGER`, but the
    /// finger left behind was already resting on the pad; acting on it would type a
    /// digit (or toggle a corner) the user never asked for.
    pub fn primary_is_new(&self) -> bool {
        match self.primary {
            Some(primary) => self.contact(primary).is_some_and(|contact| contact.is_new),
            // Single-touch pads report no lifetimes at all, so every finger-down
            // there is by definition a genuine tap.
            None => !self.multitouch,
        }
    }

    /// Closes the current `SYN_REPORT`. Call after the report has been acted on:
    /// contacts that began in it stop counting as new.
    pub fn end_report(&mut self) {
        for contact in &mut self.contacts {
            contact.is_new = false;
        }
    }

    fn contact(&self, slot: i32) -> Option<&Contact> {
        self.contacts.iter().find(|contact| contact.slot == slot)
    }

    fn contact_mut(&mut self, slot: i32) -> Option<&mut Contact> {
        self.contacts
            .iter_mut()
            .find(|contact| contact.slot == slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives one report: slot, optional tracking id, optional coordinate.
    fn report(
        tracker: &mut ContactTracker,
        slot: i32,
        tracking_id: Option<i32>,
        position: Option<(f64, f64)>,
    ) {
        tracker.set_slot(slot);
        if let Some(id) = tracking_id {
            tracker.track(id);
        }
        // Coordinates are recorded for every slot, exactly as process_event does.
        if let Some((x, y)) = position {
            tracker.update_x(x);
            tracker.update_y(y);
        }
    }

    #[test]
    fn plain_single_finger_tap_is_a_new_contact() {
        let mut tracker = ContactTracker::new();

        report(&mut tracker, 0, Some(100), Some((0.1, 0.2)));
        assert!(tracker.primary_is_new());
        assert_eq!(tracker.primary_slot(), Some(0));
        assert_eq!(tracker.primary_position().map(|p| p.x), Some(0.1));
        tracker.end_report();

        // Lift: nothing is down any more, so nothing may fire.
        report(&mut tracker, 0, Some(-1), None);
        assert!(!tracker.primary_is_new());
        assert_eq!(tracker.primary_slot(), None);
        assert!(tracker.primary_position().is_none());
    }

    #[test]
    fn does_not_fire_when_a_second_finger_survives_the_primary_lift() {
        let mut tracker = ContactTracker::new();

        // Finger A lands on "1".
        report(&mut tracker, 0, Some(100), Some((0.1, 0.9)));
        assert!(tracker.primary_is_new());
        tracker.end_report();

        // Finger B lands on "9"; the pad switches to BTN_TOOL_DOUBLETAP.
        report(&mut tracker, 1, Some(101), Some((0.9, 0.1)));
        assert_eq!(tracker.primary_slot(), Some(0));
        // A still owns the position: B must not drag it.
        assert_eq!(tracker.primary_position().map(|p| p.x), Some(0.1));
        tracker.end_report();

        // Finger A lifts. BTN_TOOL_FINGER returns to 1, but B was already resting
        // there — no key may be injected, and the position must follow B.
        report(&mut tracker, 0, Some(-1), None);
        assert_eq!(tracker.primary_slot(), Some(1));
        assert_eq!(tracker.primary_position().map(|p| p.x), Some(0.9));
        assert!(!tracker.primary_is_new());
    }

    #[test]
    fn hands_position_over_only_when_the_owning_slot_lifts() {
        let mut tracker = ContactTracker::new();

        report(&mut tracker, 0, Some(100), Some((0.1, 0.1)));
        tracker.end_report();
        report(&mut tracker, 1, Some(101), Some((0.8, 0.8)));
        tracker.end_report();

        // A secondary lift changes nothing about ownership.
        tracker.set_slot(1);
        assert!(!tracker.track(-1));
        assert_eq!(tracker.primary_slot(), Some(0));

        // The primary lift does, and there is no one left to hand over to.
        tracker.set_slot(0);
        assert!(!tracker.track(-1));
        assert_eq!(tracker.primary_slot(), None);
    }

    #[test]
    fn secondary_slot_never_moves_the_position() {
        let mut tracker = ContactTracker::new();

        report(&mut tracker, 0, Some(100), Some((0.1, 0.1)));
        tracker.end_report();

        tracker.set_slot(1);
        tracker.track(101);
        assert!(!tracker.current_slot_owns_position());

        tracker.set_slot(0);
        assert!(tracker.current_slot_owns_position());
    }

    #[test]
    fn accepts_slots_out_of_order_and_a_non_zero_first_slot() {
        let mut tracker = ContactTracker::new();

        // Slot numbering is arbitrary: the first contact may be slot 3.
        report(&mut tracker, 3, Some(200), Some((0.7, 0.3)));
        assert_eq!(tracker.primary_slot(), Some(3));
        assert_eq!(tracker.primary_position().map(|p| p.x), Some(0.7));
        assert!(tracker.primary_is_new());
        tracker.end_report();

        report(&mut tracker, 0, Some(201), Some((0.2, 0.2)));
        assert_eq!(tracker.primary_slot(), Some(3));
        tracker.end_report();

        report(&mut tracker, 3, Some(-1), None);
        assert_eq!(tracker.primary_slot(), Some(0));
        assert_eq!(tracker.primary_position().map(|p| p.x), Some(0.2));
        assert!(!tracker.primary_is_new());
    }

    #[test]
    fn single_touch_pads_without_tracking_ids_keep_acting() {
        // ABS_X/ABS_Y carry no slot and no lifetime; every finger-down is a tap.
        let tracker = ContactTracker::new();

        assert!(tracker.primary_is_new());
        assert!(tracker.current_slot_owns_position());
        assert_eq!(tracker.primary_slot(), None);
    }

    #[test]
    fn survives_absurd_slot_ids_and_missing_lifts() {
        let mut tracker = ContactTracker::new();

        // Huge and negative slot ids must not index anything.
        for slot in [i32::MAX, -1, 4096] {
            tracker.set_slot(slot);
            tracker.track(1);
            tracker.update_x(0.5);
        }
        assert_eq!(tracker.primary_slot(), Some(i32::MAX));

        // Begins that never get an end must not grow the table without bound.
        for slot in 0..64 {
            tracker.set_slot(slot);
            tracker.track(slot);
        }
        assert!(tracker.contacts.len() <= MAX_TRACKED_CONTACTS);
    }

    #[test]
    fn a_repeated_begin_on_a_live_slot_counts_as_new_again() {
        let mut tracker = ContactTracker::new();

        report(&mut tracker, 0, Some(100), Some((0.1, 0.1)));
        tracker.end_report();
        assert!(!tracker.primary_is_new());

        // Missed lift: the pad reuses the slot with a fresh tracking id.
        tracker.set_slot(0);
        tracker.track(102);
        assert!(tracker.primary_is_new());
        assert_eq!(tracker.contacts.len(), 1);
    }
}
