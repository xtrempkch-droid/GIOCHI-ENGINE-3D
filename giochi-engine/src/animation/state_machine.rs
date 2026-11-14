//! Event-driven animation state machine.

use super::{AnimationError, MAX_STATE_TRANSITIONS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Transition {
    from: u32,
    event: u32,
    to: u32,
}

/// State selector driven by explicit event transitions.
///
/// State and event values are application-defined `u32` identifiers. The machine is
/// intentionally independent of any particular animation clip; callers select a clip
/// from [`AnimationStateMachine::current_state`].
///
/// # Examples
///
/// ```
/// use giochi_engine::animation::AnimationStateMachine;
///
/// let mut states = AnimationStateMachine::new(0);
/// states.add_transition(0, 1, 2)?;
/// assert!(states.trigger(1));
/// assert_eq!(states.current_state(), 2);
/// # Ok::<(), giochi_engine::animation::AnimationError>(())
/// ```
#[repr(C)]
#[derive(Debug)]
pub struct AnimationStateMachine {
    current_state: u32,
    transitions: Vec<Transition>,
    _reserved: [u32; 4],
}

impl AnimationStateMachine {
    /// Creates a state machine at `initial_state`.
    ///
    /// # Examples
    ///
    /// ```
    /// use giochi_engine::animation::AnimationStateMachine;
    ///
    /// let states = AnimationStateMachine::new(7);
    /// assert_eq!(states.current_state(), 7);
    /// ```
    pub fn new(initial_state: u32) -> Self {
        Self {
            current_state: initial_state,
            transitions: Vec::new(),
            _reserved: [0; 4],
        }
    }

    /// Adds one transition. A state may have only one destination for a given event.
    ///
    /// # Examples
    ///
    /// ```
    /// use giochi_engine::animation::AnimationStateMachine;
    ///
    /// let mut states = AnimationStateMachine::new(0);
    /// states.add_transition(0, 10, 1)?;
    /// assert_eq!(states.transition_count(), 1);
    /// # Ok::<(), giochi_engine::animation::AnimationError>(())
    /// ```
    pub fn add_transition(&mut self, from: u32, event: u32, to: u32) -> Result<(), AnimationError> {
        if self
            .transitions
            .iter()
            .any(|transition| transition.from == from && transition.event == event)
        {
            return Err(AnimationError::DuplicateTransition);
        }
        if self.transitions.len() >= MAX_STATE_TRANSITIONS {
            return Err(AnimationError::TransitionLimitExceeded);
        }
        self.transitions.push(Transition { from, event, to });
        Ok(())
    }

    /// Applies `event` if the current state has a matching transition.
    ///
    /// Returns `true` only when the state changes.
    ///
    /// # Examples
    ///
    /// ```
    /// use giochi_engine::animation::AnimationStateMachine;
    ///
    /// let mut states = AnimationStateMachine::new(0);
    /// states.add_transition(0, 5, 1)?;
    /// assert!(states.trigger(5));
    /// assert!(!states.trigger(5));
    /// # Ok::<(), giochi_engine::animation::AnimationError>(())
    /// ```
    pub fn trigger(&mut self, event: u32) -> bool {
        let next = self
            .transitions
            .iter()
            .find(|transition| transition.from == self.current_state && transition.event == event)
            .map(|transition| transition.to);
        let Some(next) = next else {
            return false;
        };
        self.current_state = next;
        true
    }

    /// Current application-defined state identifier.
    pub fn current_state(&self) -> u32 {
        self.current_state
    }

    /// Number of registered transitions.
    pub fn transition_count(&self) -> usize {
        self.transitions.len()
    }

    /// Resets the machine to a state without changing its transition table.
    pub fn reset(&mut self, initial_state: u32) {
        self.current_state = initial_state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_only_transition_from_the_current_state() {
        let mut states = AnimationStateMachine::new(0);
        states.add_transition(0, 1, 2).expect("valid transition");
        states.add_transition(2, 1, 3).expect("valid transition");

        assert!(states.trigger(1));
        assert_eq!(states.current_state(), 2);
        assert!(states.trigger(1));
        assert_eq!(states.current_state(), 3);
        assert!(!states.trigger(1));
    }

    #[test]
    fn duplicate_state_events_are_rejected() {
        let mut states = AnimationStateMachine::new(0);
        states.add_transition(0, 1, 2).expect("valid transition");

        assert_eq!(
            states.add_transition(0, 1, 3),
            Err(AnimationError::DuplicateTransition)
        );
        assert_eq!(states.transition_count(), 1);
    }

    #[test]
    fn reset_changes_only_the_current_state() {
        let mut states = AnimationStateMachine::new(0);
        states.add_transition(0, 1, 2).expect("valid transition");
        states.reset(7);

        assert_eq!(states.current_state(), 7);
        assert_eq!(states.transition_count(), 1);
        assert!(!states.trigger(1));
    }
}
