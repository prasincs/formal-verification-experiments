//! Touch input types
//!
//! Provides common touch event types that can be used across
//! different touch controller implementations.

/// Touch point with screen coordinates
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TouchPoint {
    /// X coordinate
    pub x: u16,
    /// Y coordinate
    pub y: u16,
    /// Pressure (0 = no touch, higher = more pressure)
    pub pressure: u16,
}

impl TouchPoint {
    /// Create a new touch point
    pub const fn new(x: u16, y: u16, pressure: u16) -> Self {
        Self { x, y, pressure }
    }

    /// Create a touch point with default pressure
    pub const fn at(x: u16, y: u16) -> Self {
        Self { x, y, pressure: 255 }
    }
}

/// Touch event types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchEvent {
    /// Finger touched the screen
    Down(TouchPoint),
    /// Finger moved while touching
    Move(TouchPoint),
    /// Finger lifted from screen
    Up,
}

impl TouchEvent {
    /// Get the touch point if this is a Down or Move event
    pub fn point(&self) -> Option<TouchPoint> {
        match self {
            TouchEvent::Down(p) | TouchEvent::Move(p) => Some(*p),
            TouchEvent::Up => None,
        }
    }

    /// Check if this is a touch down event
    pub fn is_down(&self) -> bool {
        matches!(self, TouchEvent::Down(_))
    }

    /// Check if this is a move event
    pub fn is_move(&self) -> bool {
        matches!(self, TouchEvent::Move(_))
    }

    /// Check if this is a touch up event
    pub fn is_up(&self) -> bool {
        matches!(self, TouchEvent::Up)
    }
}

/// Touch controller trait for different implementations
pub trait TouchController {
    /// Check if screen is currently being touched
    fn is_touched(&self) -> bool;

    /// Read current touch point (if touched)
    fn read_point(&mut self) -> Option<TouchPoint>;

    /// Poll for touch events
    fn poll_event(&mut self) -> Option<TouchEvent>;
}

/// Gesture types that can be detected from touch sequences
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    /// Single tap
    Tap(TouchPoint),
    /// Double tap
    DoubleTap(TouchPoint),
    /// Long press
    LongPress(TouchPoint),
    /// Swipe in a direction
    Swipe(SwipeDirection, TouchPoint, TouchPoint),
    /// Pinch (for zoom)
    Pinch { center: TouchPoint, scale: i16 },
}

/// Swipe direction
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwipeDirection {
    Up,
    Down,
    Left,
    Right,
}

impl SwipeDirection {
    /// Detect swipe direction from start and end points
    pub fn from_points(start: TouchPoint, end: TouchPoint) -> Option<Self> {
        let dx = end.x as i32 - start.x as i32;
        let dy = end.y as i32 - start.y as i32;

        // Minimum swipe distance threshold
        const MIN_SWIPE: i32 = 30;

        if dx.abs() > dy.abs() {
            if dx > MIN_SWIPE {
                Some(SwipeDirection::Right)
            } else if dx < -MIN_SWIPE {
                Some(SwipeDirection::Left)
            } else {
                None
            }
        } else {
            if dy > MIN_SWIPE {
                Some(SwipeDirection::Down)
            } else if dy < -MIN_SWIPE {
                Some(SwipeDirection::Up)
            } else {
                None
            }
        }
    }
}

/// Simple gesture detector
pub struct GestureDetector {
    /// Start point of current touch
    start_point: Option<TouchPoint>,
    /// Last point during drag
    last_point: Option<TouchPoint>,
    /// Touch start time (frame count)
    start_frame: u32,
    /// Current frame
    current_frame: u32,
    /// Last tap time for double-tap detection
    last_tap_frame: u32,
    /// Last tap position
    last_tap_point: Option<TouchPoint>,
}

impl GestureDetector {
    /// Create a new gesture detector
    pub const fn new() -> Self {
        Self {
            start_point: None,
            last_point: None,
            start_frame: 0,
            current_frame: 0,
            last_tap_frame: 0,
            last_tap_point: None,
        }
    }

    /// Update the frame counter (call once per frame)
    pub fn update(&mut self) {
        self.current_frame = self.current_frame.wrapping_add(1);
    }

    /// Process a touch event and detect gestures
    pub fn process(&mut self, event: TouchEvent) -> Option<Gesture> {
        match event {
            TouchEvent::Down(point) => {
                self.start_point = Some(point);
                self.last_point = Some(point);
                self.start_frame = self.current_frame;
                None
            }

            TouchEvent::Move(point) => {
                self.last_point = Some(point);
                None
            }

            TouchEvent::Up => {
                let start = self.start_point?;
                let end = self.last_point.unwrap_or(start);
                let duration = self.current_frame.wrapping_sub(self.start_frame);

                // Clear state
                self.start_point = None;
                self.last_point = None;

                // Long press: held for > 30 frames without moving much
                const LONG_PRESS_FRAMES: u32 = 30;
                const TAP_MOVE_THRESHOLD: u16 = 20;

                let moved = ((end.x as i32 - start.x as i32).abs() as u16 > TAP_MOVE_THRESHOLD)
                    || ((end.y as i32 - start.y as i32).abs() as u16 > TAP_MOVE_THRESHOLD);

                if !moved && duration > LONG_PRESS_FRAMES {
                    return Some(Gesture::LongPress(start));
                }

                // Check for swipe
                if let Some(direction) = SwipeDirection::from_points(start, end) {
                    return Some(Gesture::Swipe(direction, start, end));
                }

                // Check for double tap
                const DOUBLE_TAP_FRAMES: u32 = 20;
                const DOUBLE_TAP_DISTANCE: u16 = 30;

                if let Some(last_tap) = self.last_tap_point {
                    let tap_interval = self.current_frame.wrapping_sub(self.last_tap_frame);
                    let tap_distance = ((start.x as i32 - last_tap.x as i32).abs() as u16)
                        .max((start.y as i32 - last_tap.y as i32).abs() as u16);

                    if tap_interval < DOUBLE_TAP_FRAMES && tap_distance < DOUBLE_TAP_DISTANCE {
                        self.last_tap_point = None;
                        return Some(Gesture::DoubleTap(start));
                    }
                }

                // Single tap
                self.last_tap_frame = self.current_frame;
                self.last_tap_point = Some(start);
                Some(Gesture::Tap(start))
            }
        }
    }
}

impl Default for GestureDetector {
    fn default() -> Self {
        Self::new()
    }
}
