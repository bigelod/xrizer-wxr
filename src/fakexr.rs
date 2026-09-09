//! Test doubles for the OpenXR runtime.
//!
//! The `winlatorxr` shim and the `#[cfg(test)]` suites use this module to inject
//! fake interaction profiles, action state, hand poses and frame state without
//! requiring a real headset. This module is only compiled for tests, and its
//! state is thread-local so that parallel tests don't interfere with each other.

pub mod vulkan;

use crate::winlatorxr::{Binding, Hand, Path, RawAction, RawSession, XrPosef, XrTime};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// The sub-action user path that a test manipulates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserPath {
    LeftHand,
    RightHand,
}

impl UserPath {
    pub fn path(self) -> Path {
        let string = match self {
            UserPath::LeftHand => "/user/hand/left",
            UserPath::RightHand => "/user/hand/right",
        };
        crate::winlatorxr::string_to_path_global(string)
    }

    pub fn hand(self) -> Hand {
        match self {
            UserPath::LeftHand => Hand::Left,
            UserPath::RightHand => Hand::Right,
        }
    }
}

/// The value of an action.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActionState {
    Bool(bool),
    Float(f32),
    Vector2(f32, f32),
}

impl From<bool> for ActionState {
    fn from(value: bool) -> Self {
        ActionState::Bool(value)
    }
}

/// The full state of an action, as read by the shim's `Action::state`.
pub struct ActionStateData {
    pub state: ActionState,
    pub changed: bool,
    pub is_active: bool,
    pub last_change_time: XrTime,
}

/// The frame state of a session, as driven by `FrameWaiter`/`FrameStream`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameState {
    Waited,
    Begun,
    Ended,
}

#[derive(Debug, Clone, Copy)]
struct ActionEntry {
    current: ActionState,
    previous_synced: ActionState,
    changed: bool,
    is_active: bool,
    last_change_time: XrTime,
}

impl Default for ActionEntry {
    fn default() -> Self {
        Self {
            current: ActionState::Bool(false),
            previous_synced: ActionState::Bool(false),
            changed: false,
            is_active: false,
            last_change_time: XrTime(0),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct HandPoses {
    grip: Option<XrPosef>,
    aim: Option<XrPosef>,
}

#[derive(Debug, Default)]
struct Shared {
    action_states: HashMap<(u64, u64), ActionEntry>,
    profiles: HashMap<(u64, u64), Path>,
    /// Profiles already applied by the runtime (as seen by `sync_actions`).
    applied_profiles: HashMap<(u64, u64), Path>,
    /// Hands whose interaction profile changed and whose
    /// `XR_TYPE_EVENT_DATA_INTERACTION_PROFILE_CHANGED` event has not been
    /// consumed by `poll_events` yet.
    pending_profiles: HashSet<(u64, u64)>,
    poses: HashMap<(u64, u64), HandPoses>,
    suggested_bindings: HashMap<(u64, u64), Vec<String>>,
    frame_states: HashMap<u64, FrameState>,
    /// Sessions that have been marked as "synchronized" (should render).
    synchronized: HashSet<u64>,
    /// Sessions that completed a frame (`FrameStream::end`) but whose
    /// synchronization has not been applied by `poll_events` yet.
    pending_synchronize: HashSet<u64>,
    sessions_with_trackers: HashSet<u64>,
    haptics: HashSet<(u64, u64)>,
}

thread_local! {
    static SHARED: RefCell<Shared> = RefCell::new(Shared::default());
}

pub fn now() -> XrTime {
    XrTime(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as i64,
    )
}

/// Record a suggested binding for a profile. Called by the shim's
/// `suggest_interaction_profile_bindings` in test builds.
pub fn suggest_bindings(profile: Path, bindings: &[Binding]) {
    SHARED.with(|shared| {
        let mut data = shared.borrow_mut();
        for binding in bindings {
            let path = crate::winlatorxr::path_to_string_global(binding.path)
                .unwrap_or_else(|| format!("{:?}", binding.path));
            data.suggested_bindings
                .entry((profile.0, binding.action.0))
                .or_default()
                .push(path);
        }
    });
}

/// Return the suggested binding paths for an action under a profile.
pub fn get_suggested_bindings(action: RawAction, profile: Path) -> Vec<String> {
    SHARED.with(|shared| {
        shared
            .borrow()
            .suggested_bindings
            .get(&(profile.0, action.0))
            .cloned()
            .unwrap_or_default()
    })
}

/// Set the interaction profile used by a hand. Called by the test fixtures.
/// The change only becomes observable via `poll_events` after the next
/// `sync_actions`, mirroring how a real runtime reports
/// `XR_TYPE_EVENT_DATA_INTERACTION_PROFILE_CHANGED`.
pub fn set_interaction_profile(session: RawSession, hand: UserPath, profile: Path) {
    SHARED.with(|shared| {
        shared
            .borrow_mut()
            .profiles
            .insert((session.0, hand.path().0), profile);
    });
}

/// Whether an interaction profile was set for this session since the last
/// `clear_profile_changes` call. Used by `poll_events` to emulate the
/// `XR_TYPE_EVENT_DATA_INTERACTION_PROFILE_CHANGED` runtime event.
pub fn has_profile_changes(session: RawSession) -> bool {
    SHARED.with(|shared| {
        shared
            .borrow()
            .pending_profiles
            .iter()
            .any(|(s, _)| *s == session.0)
    })
}

/// Clear the pending interaction profile changes for a session, recording the
/// current profiles as applied.
pub fn clear_profile_changes(session: RawSession) {
    SHARED.with(|shared| {
        let mut data = shared.borrow_mut();
        data.pending_profiles.retain(|(s, _)| *s != session.0);
        let applied: Vec<((u64, u64), Path)> = data
            .profiles
            .iter()
            .filter(|((s, _), _)| *s == session.0)
            .map(|(&key, profile)| (key, *profile))
            .collect();
        for (key, profile) in applied {
            data.applied_profiles.insert(key, profile);
        }
    });
}

/// Return the interaction profile of a user path, if one was set.
pub fn current_interaction_profile(
    session: RawSession,
    top_level_user_path: Path,
) -> Option<Path> {
    SHARED.with(|shared| {
        shared
            .borrow()
            .profiles
            .get(&(session.0, top_level_user_path.0))
            .copied()
    })
}

/// Set the grip pose of a hand.
pub fn set_grip(session: RawSession, hand: UserPath, pose: XrPosef) {
    SHARED.with(|shared| {
        let mut data = shared.borrow_mut();
        let poses = data
            .poses
            .entry((session.0, hand.hand() as u64))
            .or_default();
        poses.grip = Some(pose);
    });
}

/// Set the aim pose of a hand.
pub fn set_aim(session: RawSession, hand: UserPath, pose: XrPosef) {
    SHARED.with(|shared| {
        let mut data = shared.borrow_mut();
        let poses = data
            .poses
            .entry((session.0, hand.hand() as u64))
            .or_default();
        poses.aim = Some(pose);
    });
}

fn hand_from_path(path: Path) -> Option<Hand> {
    match crate::winlatorxr::path_to_string_global(path).as_deref() {
        Some("/user/hand/left") => Some(Hand::Left),
        Some("/user/hand/right") => Some(Hand::Right),
        _ => None,
    }
}

/// Return the grip pose of the hand referenced by `hand_path`, if set.
/// Used by `Space::relate` for action spaces in test builds.
pub fn get_pose_for_space(session: RawSession, hand_path: Path) -> Option<XrPosef> {
    let hand = hand_from_path(hand_path)?;
    SHARED.with(|shared| {
        shared
            .borrow()
            .poses
            .get(&(session.0, hand as u64))
            .and_then(|poses| poses.grip)
    })
}

fn set_action_state_impl(
    action: RawAction,
    state: ActionState,
    subaction: UserPath,
    time: XrTime,
) {
    let key = (action.0, subaction.path().0);
    SHARED.with(|shared| {
        let mut data = shared.borrow_mut();
        let entry = data.action_states.entry(key).or_default();
        entry.current = state;
        entry.is_active = true;
        entry.last_change_time = time;
    });
}

/// Set the state of an action for a sub-action path. Called by the test
/// fixtures before `sync_actions` is invoked.
pub fn set_action_state(action: RawAction, state: ActionState, subaction: UserPath) {
    set_action_state_impl(action, state, subaction, now());
}

/// Set the state of an action with an explicit timestamp.
pub fn set_action_state_with_time(
    action: RawAction,
    state: ActionState,
    subaction: UserPath,
    time: XrTime,
) {
    set_action_state_impl(action, state, subaction, time);
}

/// Read the state of an action. Called by the shim's `Action::state` in test
/// builds.
pub fn get_action_state(action_path: Path, subaction_path: Path) -> Option<ActionStateData> {
    SHARED.with(|shared| {
        let data = shared.borrow();
        if subaction_path == Path::NULL {
            // A null sub-action path means "any hand": fall back to the state
            // of whichever sub-action has one recorded for this action.
            let entry = data
                .action_states
                .iter()
                .find(|((a, _), _)| *a == action_path.0)
                .map(|(_, e)| e)?;
            return Some(ActionStateData {
                state: entry.current,
                changed: entry.changed,
                is_active: entry.is_active,
                last_change_time: entry.last_change_time,
            });
        }
        let entry = data.action_states.get(&(action_path.0, subaction_path.0))?;
        Some(ActionStateData {
            state: entry.current,
            changed: entry.changed,
            is_active: entry.is_active,
            last_change_time: entry.last_change_time,
        })
    })
}

/// Whether an action is active for a sub-action path. Called by the shim's
/// `Action::is_active` in test builds.
pub fn action_is_active(
    action_path: Path,
    subaction_path: Path,
    session: RawSession,
) -> Option<bool> {
    SHARED.with(|shared| {
        let data = shared.borrow();
        if subaction_path == Path::NULL {
            let has_pose = data.poses.keys().any(|(s, _)| *s == session.0);
            let has_state = data.action_states.keys().any(|(a, _)| *a == action_path.0);
            Some(has_pose || has_state)
        } else {
            data.action_states
                .get(&(action_path.0, subaction_path.0))
                .map(|entry| entry.is_active)
        }
    })
}

/// Called by the shim's `Session::sync_actions` in test builds. Commits the
/// pending action states: the `changed` flag reflects whether the current state
/// differs from the one seen at the previous sync. Also queues interaction
/// profile changed events for any hand whose profile differs from the one that
/// was previously applied.
pub fn sync_actions(session: RawSession) {
    SHARED.with(|shared| {
        let mut data = shared.borrow_mut();
        for entry in data.action_states.values_mut() {
            entry.changed = entry.current != entry.previous_synced;
            entry.previous_synced = entry.current;
        }
        let mut pending: Vec<(u64, u64)> = Vec::new();
        for (&key, profile) in data.profiles.iter() {
            if key.0 == session.0 && data.applied_profiles.get(&key) != Some(profile) {
                pending.push(key);
            }
        }
        data.pending_profiles.extend(pending);
    });
}

/// Record a haptic pulse. Called by the shim's `Action::apply_feedback` in test
/// builds; `is_haptic_activated` then reports whether one is in flight.
pub fn trigger_haptic(action_path: Path, subaction_path: Path) {
    SHARED.with(|shared| {
        shared
            .borrow_mut()
            .haptics
            .insert((action_path.0, subaction_path.0));
    });
}

/// Whether a haptic pulse was triggered for an action and hand.
pub fn is_haptic_activated(action: RawAction, hand: UserPath) -> bool {
    SHARED.with(|shared| {
        shared
            .borrow()
            .haptics
            .contains(&(action.0, hand.path().0))
    })
}

/// Whether no suggested bindings were recorded for an action under a profile.
pub fn check_no_suggested_bindings(action: RawAction, profile: Path) -> bool {
    SHARED.with(|shared| {
        !shared
            .borrow()
            .suggested_bindings
            .contains_key(&(profile.0, action.0))
    })
}

/// Mark a session as having extra trackers. The tracker-specific tests are
/// gated behind the `monado` feature and are ignored by default.
pub fn add_trackers(session: RawSession) {
    SHARED.with(|shared| {
        shared.borrow_mut().sessions_with_trackers.insert(session.0);
    });
}

/// Set the frame state of a session. Called by `FrameWaiter`/`FrameStream` in
/// test builds.
pub fn set_frame_state(session: RawSession, state: FrameState) {
    SHARED.with(|shared| {
        shared.borrow_mut().frame_states.insert(session.0, state);
    });
}

/// Read the frame state of a session.
pub fn session_frame_state(session: RawSession) -> FrameState {
    SHARED.with(|shared| {
        shared
            .borrow()
            .frame_states
            .get(&session.0)
            .copied()
            .unwrap_or(FrameState::Ended)
    })
}

/// Whether the session has been synchronized (i.e. whether the app should
/// render). False until the first `FrameStream::end` has been applied by
/// `poll_events`.
pub fn session_synchronized(session: RawSession) -> bool {
    SHARED.with(|shared| shared.borrow().synchronized.contains(&session.0))
}

/// Mark the session as pending synchronization. Called by `FrameStream::end`
/// in test builds.
pub fn queue_synchronize(session: RawSession) {
    SHARED.with(|shared| {
        shared.borrow_mut().pending_synchronize.insert(session.0);
    });
}

/// Apply any queued synchronization for the session. Called by `poll_events`
/// in test builds, mirroring when the runtime would deliver the
/// `XR_TYPE_EVENT_DATA_SESSION_STATE_CHANGED` event.
pub fn apply_synchronize(session: RawSession) {
    SHARED.with(|shared| {
        let mut data = shared.borrow_mut();
        if data.pending_synchronize.remove(&session.0) {
            data.synchronized.insert(session.0);
        }
    });
}
