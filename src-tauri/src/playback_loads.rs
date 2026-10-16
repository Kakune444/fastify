use std::collections::VecDeque;

/// Associates each native Load acknowledgement with the UI selection that sent it.
#[derive(Default)]
pub struct PlaybackLoads {
    pending: VecDeque<String>,
    active: Option<(u64, String)>,
}
impl PlaybackLoads {
    pub fn push(&mut self, selection: String) {
        self.pending.push_back(selection);
    }
    pub fn changed(&mut self, native_id: u64) {
        if self.active.as_ref().is_some_and(|(id, _)| *id == native_id) {
            return;
        }
        self.active = self
            .pending
            .pop_front()
            .map(|selection| (native_id, selection));
    }
    pub fn accepts(&self, native_id: u64, selection: &str) -> bool {
        self.active
            .as_ref()
            .is_some_and(|(id, active)| *id == native_id && active == selection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_end_events_cannot_end_a_new_selection_of_the_same_track() {
        let mut loads = PlaybackLoads::default();
        loads.push("first".into());
        loads.push("second".into());
        loads.changed(10);
        assert!(!loads.accepts(10, "second"));
        loads.changed(11);
        assert!(loads.accepts(11, "second"));
        assert!(!loads.accepts(10, "second"));
    }
    #[test]
    fn repeated_acknowledgements_do_not_consume_the_next_load() {
        let mut loads = PlaybackLoads::default();
        loads.push("first".into());
        loads.changed(0);
        loads.push("second".into());
        loads.changed(0);
        assert!(loads.accepts(0, "first"));
        loads.changed(2);
        assert!(loads.accepts(2, "second"));
        loads.changed(3);
        assert!(!loads.accepts(3, "second"));
    }
}
