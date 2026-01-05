//! TV Demo Application
//!
//! Main application that combines menu navigation and animation playback.

use crate::display::{Framebuffer, Rgb565};
use crate::input::{InputEvent, KeyCode, KeyState, IrButton};
use crate::touch::TouchEvent;

use super::animation::{AnimationPlayer, AnimationType};
use super::menu::{Menu, MenuItem, MenuStyle};

/// Demo application state
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DemoState {
    /// Showing main menu
    Menu,
    /// Playing animation
    Playing,
    /// Paused animation
    Paused,
    /// Settings screen
    Settings,
}

/// Current screen/view
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    MainMenu,
    AnimationSelect,
    NowPlaying,
    Settings,
    About,
}

/// Menu item IDs
mod menu_ids {
    pub const PLAY_ANIMATION: u8 = 1;
    pub const SELECT_ANIMATION: u8 = 2;
    pub const SETTINGS: u8 = 3;
    pub const ABOUT: u8 = 4;

    // Animation selection
    pub const ANIM_BOUNCING_BALL: u8 = 10;
    pub const ANIM_COLOR_CYCLE: u8 = 11;
    pub const ANIM_SPINNER: u8 = 12;
    pub const ANIM_BACK: u8 = 19;

    // Settings
    pub const SETTING_THEME: u8 = 20;
    pub const SETTING_SPEED: u8 = 21;
    pub const SETTING_BACK: u8 = 29;
}

/// TV Demo application
pub struct TvDemo {
    /// Current state
    state: DemoState,
    /// Current screen
    screen: Screen,
    /// Main menu
    main_menu: Menu,
    /// Animation selection menu
    anim_menu: Menu,
    /// Settings menu
    settings_menu: Menu,
    /// Animation player
    player: AnimationPlayer,
    /// Show overlay (controls hint)
    show_overlay: bool,
    /// Overlay timeout counter
    overlay_timer: u16,
    /// Dark theme enabled
    dark_theme: bool,
}

impl TvDemo {
    /// Create a new TV demo application
    pub fn new() -> Self {
        let mut demo = Self {
            state: DemoState::Menu,
            screen: Screen::MainMenu,
            main_menu: Menu::new(),
            anim_menu: Menu::new(),
            settings_menu: Menu::new(),
            player: AnimationPlayer::new(),
            show_overlay: false,
            overlay_timer: 0,
            dark_theme: true,
        };

        demo.setup_menus();
        demo
    }

    /// Set up all menus
    fn setup_menus(&mut self) {
        // Main menu
        self.main_menu.set_title("TV Demo");
        self.main_menu.add_item(MenuItem::with_label(menu_ids::PLAY_ANIMATION, "Play Animation"));
        self.main_menu.add_item(MenuItem::with_label(menu_ids::SELECT_ANIMATION, "Select Animation"));
        self.main_menu.add_item(MenuItem::with_label(menu_ids::SETTINGS, "Settings"));
        self.main_menu.add_item(MenuItem::with_label(menu_ids::ABOUT, "About"));

        // Animation selection menu
        self.anim_menu.set_title("Select Animation");
        self.anim_menu.add_item(MenuItem::with_label(menu_ids::ANIM_BOUNCING_BALL, "Bouncing Ball"));
        self.anim_menu.add_item(MenuItem::with_label(menu_ids::ANIM_COLOR_CYCLE, "Color Cycle"));
        self.anim_menu.add_item(MenuItem::with_label(menu_ids::ANIM_SPINNER, "Loading Spinner"));
        self.anim_menu.add_item(MenuItem::with_label(menu_ids::ANIM_BACK, "< Back"));

        // Settings menu
        self.settings_menu.set_title("Settings");
        self.settings_menu.add_item(MenuItem::with_label(menu_ids::SETTING_THEME, "Theme: Dark"));
        self.settings_menu.add_item(MenuItem::with_label(menu_ids::SETTING_SPEED, "Speed: Normal"));
        self.settings_menu.add_item(MenuItem::with_label(menu_ids::SETTING_BACK, "< Back"));

        self.apply_theme();
    }

    /// Apply current theme to menus
    fn apply_theme(&mut self) {
        let style = if self.dark_theme {
            MenuStyle::dark()
        } else {
            MenuStyle::light()
        };

        self.main_menu.set_style(style);
        self.anim_menu.set_style(style);
        self.settings_menu.set_style(style);
    }

    /// Get current state
    pub fn state(&self) -> DemoState {
        self.state
    }

    /// Get current screen
    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// Handle input event
    pub fn handle_input(&mut self, event: InputEvent) {
        match event {
            InputEvent::Key(key_event) => {
                if key_event.state == KeyState::Pressed {
                    self.handle_key(key_event.key);
                }
            }
            InputEvent::Remote(ir_event) => {
                if !ir_event.is_repeat {
                    self.handle_ir_button(ir_event.button);
                }
            }
            InputEvent::Touch(touch_event) => {
                self.handle_touch(touch_event);
            }
        }
    }

    /// Handle keyboard input
    fn handle_key(&mut self, key: KeyCode) {
        match self.state {
            DemoState::Menu => self.handle_menu_key(key),
            DemoState::Playing | DemoState::Paused => self.handle_playback_key(key),
            DemoState::Settings => self.handle_menu_key(key),
        }
    }

    /// Handle IR remote button
    fn handle_ir_button(&mut self, button: IrButton) {
        // Map IR buttons to actions
        match button {
            IrButton::Up => self.handle_key(KeyCode::Up),
            IrButton::Down => self.handle_key(KeyCode::Down),
            IrButton::Left => self.handle_key(KeyCode::Left),
            IrButton::Right => self.handle_key(KeyCode::Right),
            IrButton::Ok => self.handle_key(KeyCode::Enter),
            IrButton::Back | IrButton::Menu => self.handle_key(KeyCode::Escape),
            IrButton::Play | IrButton::Pause => self.handle_key(KeyCode::Space),
            IrButton::Stop => {
                if self.state == DemoState::Playing || self.state == DemoState::Paused {
                    self.stop_playback();
                }
            }
            IrButton::SkipNext => self.handle_key(KeyCode::Right),
            IrButton::SkipPrev => self.handle_key(KeyCode::Left),
            IrButton::Home => {
                self.screen = Screen::MainMenu;
                self.state = DemoState::Menu;
            }
            _ => {}
        }
    }

    /// Handle touch input
    fn handle_touch(&mut self, event: TouchEvent) {
        match event {
            TouchEvent::Down(point) => {
                // Show overlay when touched during playback
                if self.state == DemoState::Playing {
                    self.show_overlay = true;
                    self.overlay_timer = 180; // 3 seconds at 60fps
                }

                // Simple touch zones for menu navigation
                match self.state {
                    DemoState::Menu | DemoState::Settings => {
                        // Calculate which menu item was touched
                        let menu = self.current_menu();
                        if let Some(menu) = menu {
                            let item_height = 32u16;
                            let start_y = 40u16;
                            let item_idx = (point.y.saturating_sub(start_y)) / item_height;

                            if (item_idx as usize) < menu.item_count() {
                                // Navigate to item and select
                                while menu.selected_index() != item_idx as usize {
                                    if menu.selected_index() < item_idx as usize {
                                        menu.move_down();
                                    } else {
                                        menu.move_up();
                                    }
                                }
                            }
                        }
                    }
                    DemoState::Playing | DemoState::Paused => {
                        // Touch center to toggle play/pause
                        if point.x > 100 && point.x < 220 && point.y > 80 && point.y < 160 {
                            self.toggle_playback();
                        }
                        // Touch left side for previous
                        else if point.x < 80 {
                            self.player.prev();
                        }
                        // Touch right side for next
                        else if point.x > 240 {
                            self.player.next();
                        }
                    }
                }
            }
            TouchEvent::Up => {
                // Could trigger selection on release
            }
            TouchEvent::Move(_) => {
                // Could implement swipe gestures
            }
        }
    }

    /// Get mutable reference to current menu
    fn current_menu(&mut self) -> Option<&mut Menu> {
        match self.screen {
            Screen::MainMenu => Some(&mut self.main_menu),
            Screen::AnimationSelect => Some(&mut self.anim_menu),
            Screen::Settings => Some(&mut self.settings_menu),
            _ => None,
        }
    }

    /// Handle menu navigation keys
    fn handle_menu_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Up => {
                if let Some(menu) = self.current_menu() {
                    menu.move_up();
                }
            }
            KeyCode::Down => {
                if let Some(menu) = self.current_menu() {
                    menu.move_down();
                }
            }
            KeyCode::Enter | KeyCode::Space => {
                self.select_current_item();
            }
            KeyCode::Escape => {
                self.go_back();
            }
            _ => {}
        }
    }

    /// Handle playback control keys
    fn handle_playback_key(&mut self, key: KeyCode) {
        // Show overlay on any key
        self.show_overlay = true;
        self.overlay_timer = 180;

        match key {
            KeyCode::Space | KeyCode::PlayPause => {
                self.toggle_playback();
            }
            KeyCode::Escape => {
                self.stop_playback();
            }
            KeyCode::Left | KeyCode::PrevTrack => {
                self.player.prev();
            }
            KeyCode::Right | KeyCode::NextTrack => {
                self.player.next();
            }
            KeyCode::Stop => {
                self.stop_playback();
            }
            _ => {}
        }
    }

    /// Select current menu item
    fn select_current_item(&mut self) {
        let selected_id = match self.screen {
            Screen::MainMenu => self.main_menu.selected_item().map(|i| i.id),
            Screen::AnimationSelect => self.anim_menu.selected_item().map(|i| i.id),
            Screen::Settings => self.settings_menu.selected_item().map(|i| i.id),
            _ => None,
        };

        if let Some(id) = selected_id {
            match id {
                // Main menu actions
                menu_ids::PLAY_ANIMATION => {
                    self.start_playback();
                }
                menu_ids::SELECT_ANIMATION => {
                    self.screen = Screen::AnimationSelect;
                }
                menu_ids::SETTINGS => {
                    self.screen = Screen::Settings;
                    self.state = DemoState::Settings;
                }
                menu_ids::ABOUT => {
                    self.screen = Screen::About;
                }

                // Animation selection
                menu_ids::ANIM_BOUNCING_BALL => {
                    self.player.play(AnimationType::BouncingBall);
                    self.start_playback();
                }
                menu_ids::ANIM_COLOR_CYCLE => {
                    self.player.play(AnimationType::ColorCycle);
                    self.start_playback();
                }
                menu_ids::ANIM_SPINNER => {
                    self.player.play(AnimationType::Spinner);
                    self.start_playback();
                }
                menu_ids::ANIM_BACK => {
                    self.go_back();
                }

                // Settings
                menu_ids::SETTING_THEME => {
                    self.dark_theme = !self.dark_theme;
                    self.apply_theme();
                }
                menu_ids::SETTING_BACK => {
                    self.go_back();
                }

                _ => {}
            }
        }
    }

    /// Go back to previous screen
    fn go_back(&mut self) {
        match self.screen {
            Screen::AnimationSelect | Screen::Settings | Screen::About => {
                self.screen = Screen::MainMenu;
                self.state = DemoState::Menu;
            }
            Screen::NowPlaying => {
                self.stop_playback();
            }
            Screen::MainMenu => {
                // Already at main menu
            }
        }
    }

    /// Start animation playback
    fn start_playback(&mut self) {
        self.state = DemoState::Playing;
        self.screen = Screen::NowPlaying;
        self.player.play(self.player.current());
        self.show_overlay = true;
        self.overlay_timer = 180;
    }

    /// Stop animation playback
    fn stop_playback(&mut self) {
        self.player.stop();
        self.state = DemoState::Menu;
        self.screen = Screen::MainMenu;
        self.show_overlay = false;
    }

    /// Toggle play/pause
    fn toggle_playback(&mut self) {
        self.player.toggle();
        self.state = if self.player.is_playing() {
            DemoState::Playing
        } else {
            DemoState::Paused
        };
    }

    /// Update application state (call each frame)
    pub fn update(&mut self) {
        // Update overlay timer
        if self.overlay_timer > 0 {
            self.overlay_timer -= 1;
            if self.overlay_timer == 0 {
                self.show_overlay = false;
            }
        }

        // Update animation if playing
        if self.state == DemoState::Playing {
            self.player.update();
        }
    }

    /// Render current view to framebuffer
    pub fn render(&self, fb: &mut Framebuffer) {
        match self.screen {
            Screen::MainMenu => {
                self.main_menu.render(fb);
            }
            Screen::AnimationSelect => {
                self.anim_menu.render(fb);
            }
            Screen::Settings => {
                self.settings_menu.render(fb);
            }
            Screen::About => {
                self.render_about(fb);
            }
            Screen::NowPlaying => {
                self.player.render(fb);

                if self.show_overlay {
                    self.render_playback_overlay(fb);
                }
            }
        }
    }

    /// Render about screen
    fn render_about(&self, fb: &mut Framebuffer) {
        let bg = if self.dark_theme {
            Rgb565::from_rgb(20, 20, 30)
        } else {
            Rgb565::WHITE
        };
        let fg = if self.dark_theme {
            Rgb565::WHITE
        } else {
            Rgb565::BLACK
        };

        fb.clear(bg);

        // Title bar
        fb.fill_rect(0, 0, 320, 30, Rgb565::from_rgb(40, 80, 160));

        // Content area - placeholder for text
        // "TV Demo v1.0"
        fb.fill_rect(100, 80, 120, 10, fg);
        // "Verified with Verus"
        fb.fill_rect(80, 110, 160, 10, fg);
        // "Press Back to return"
        fb.fill_rect(70, 180, 180, 10, Rgb565::from_rgb(100, 100, 100));
    }

    /// Render playback overlay with controls
    fn render_playback_overlay(&self, fb: &mut Framebuffer) {
        // Semi-transparent bar at bottom
        let bar_y = 200u16;
        let bar_color = Rgb565::from_rgb(30, 30, 50);

        fb.fill_rect(0, bar_y, 320, 40, bar_color);

        // Play/Pause indicator
        let indicator_x = 150u16;
        let indicator_y = bar_y + 10;

        if self.player.is_playing() {
            // Pause icon (two bars)
            fb.fill_rect(indicator_x, indicator_y, 8, 20, Rgb565::WHITE);
            fb.fill_rect(indicator_x + 12, indicator_y, 8, 20, Rgb565::WHITE);
        } else {
            // Play icon (triangle approximation)
            for i in 0..20u16 {
                let width = i.min(20 - i) + 1;
                fb.fill_rect(indicator_x + i, indicator_y + (10 - width / 2), 1, width, Rgb565::WHITE);
            }
        }

        // Previous button
        fb.fill_rect(80, indicator_y + 5, 10, 10, Rgb565::from_rgb(150, 150, 150));

        // Next button
        fb.fill_rect(230, indicator_y + 5, 10, 10, Rgb565::from_rgb(150, 150, 150));

        // Animation name indicator (top)
        let name_color = Rgb565::from_rgb(200, 200, 200);
        let name_width = match self.player.current() {
            AnimationType::BouncingBall => 96,  // "Bouncing Ball"
            AnimationType::ColorCycle => 88,    // "Color Cycle"
            AnimationType::Spinner => 56,       // "Spinner"
        };
        fb.fill_rect((320 - name_width) / 2, 10, name_width, 8, name_color);
    }
}

impl Default for TvDemo {
    fn default() -> Self {
        Self::new()
    }
}
