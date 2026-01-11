//! TV Demo Application
//!
//! Main application that combines menu navigation and animation playback.

use crate::backend::{DisplayBackend, Color};
use crate::animation::{AnimationPlayer, AnimationType};
use crate::menu::{Menu, MenuItem, MenuStyle};
use rpi4_input::{InputEvent, KeyCode, KeyState, IrButton, TouchEvent};

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

    pub const ANIM_BOUNCING_BALL: u8 = 10;
    pub const ANIM_COLOR_CYCLE: u8 = 11;
    pub const ANIM_SPINNER: u8 = 12;
    pub const ANIM_BACK: u8 = 19;

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
    /// Screen dimensions
    width: u32,
    height: u32,
}

impl TvDemo {
    /// Create a new TV demo application
    pub fn new(width: u32, height: u32) -> Self {
        let mut demo = Self {
            state: DemoState::Menu,
            screen: Screen::MainMenu,
            main_menu: Menu::new(width, height),
            anim_menu: Menu::new(width, height),
            settings_menu: Menu::new(width, height),
            player: AnimationPlayer::new(width, height),
            show_overlay: false,
            overlay_timer: 0,
            dark_theme: true,
            width,
            height,
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
                if self.state == DemoState::Playing {
                    self.show_overlay = true;
                    self.overlay_timer = 180;
                }

                match self.state {
                    DemoState::Menu | DemoState::Settings => {
                        let style = MenuStyle::dark();
                        let item_idx = point.y.saturating_sub(style.padding_top as u16) / style.item_height as u16;
                        let menu = match self.screen {
                            Screen::MainMenu => &mut self.main_menu,
                            Screen::AnimationSelect => &mut self.anim_menu,
                            Screen::Settings => &mut self.settings_menu,
                            _ => return,
                        };

                        if (item_idx as usize) < menu.item_count() {
                            while menu.selected_index() != item_idx as usize {
                                if menu.selected_index() < item_idx as usize {
                                    menu.move_down();
                                } else {
                                    menu.move_up();
                                }
                            }
                        }
                    }
                    DemoState::Playing | DemoState::Paused => {
                        let center_x = self.width / 2;
                        let margin = self.width / 4;

                        if point.x as u32 > center_x - margin / 2 && (point.x as u32) < center_x + margin / 2 {
                            self.toggle_playback();
                        } else if (point.x as u32) < margin {
                            self.player.prev();
                        } else if point.x as u32 > self.width - margin {
                            self.player.next();
                        }
                    }
                }
            }
            TouchEvent::Up => {}
            TouchEvent::Move(_) => {}
        }
    }

    /// Handle menu navigation keys
    fn handle_menu_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Up => {
                match self.screen {
                    Screen::MainMenu => self.main_menu.move_up(),
                    Screen::AnimationSelect => self.anim_menu.move_up(),
                    Screen::Settings => self.settings_menu.move_up(),
                    _ => {}
                }
            }
            KeyCode::Down => {
                match self.screen {
                    Screen::MainMenu => self.main_menu.move_down(),
                    Screen::AnimationSelect => self.anim_menu.move_down(),
                    Screen::Settings => self.settings_menu.move_down(),
                    _ => {}
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
                menu_ids::PLAY_ANIMATION => self.start_playback(),
                menu_ids::SELECT_ANIMATION => self.screen = Screen::AnimationSelect,
                menu_ids::SETTINGS => {
                    self.screen = Screen::Settings;
                    self.state = DemoState::Settings;
                }
                menu_ids::ABOUT => self.screen = Screen::About,

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
                menu_ids::ANIM_BACK => self.go_back(),

                menu_ids::SETTING_THEME => {
                    self.dark_theme = !self.dark_theme;
                    self.apply_theme();
                }
                menu_ids::SETTING_BACK => self.go_back(),

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
            Screen::NowPlaying => self.stop_playback(),
            Screen::MainMenu => {}
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
        if self.overlay_timer > 0 {
            self.overlay_timer -= 1;
            if self.overlay_timer == 0 {
                self.show_overlay = false;
            }
        }

        if self.state == DemoState::Playing {
            self.player.update();
        }
    }

    /// Render current view to display
    pub fn render<D: DisplayBackend>(&self, display: &mut D) {
        match self.screen {
            Screen::MainMenu => self.main_menu.render(display),
            Screen::AnimationSelect => self.anim_menu.render(display),
            Screen::Settings => self.settings_menu.render(display),
            Screen::About => self.render_about(display),
            Screen::NowPlaying => {
                self.player.render(display);
                if self.show_overlay {
                    self.render_playback_overlay(display);
                }
            }
        }
    }

    /// Render about screen
    fn render_about<D: DisplayBackend>(&self, display: &mut D) {
        let bg = if self.dark_theme {
            Color::rgb(20, 20, 30)
        } else {
            Color::WHITE
        };
        let fg = if self.dark_theme {
            Color::WHITE
        } else {
            Color::BLACK
        };

        display.clear(bg);
        display.fill_rect(0, 0, self.width, 30, Color::rgb(40, 80, 160));

        // Text placeholders
        let cx = self.width / 2;
        display.fill_rect(cx - 60, self.height / 3, 120, 10, fg);
        display.fill_rect(cx - 80, self.height / 2, 160, 10, fg);
        display.fill_rect(cx - 90, self.height * 3 / 4, 180, 10, Color::GRAY);
    }

    /// Render playback overlay with controls
    fn render_playback_overlay<D: DisplayBackend>(&self, display: &mut D) {
        let bar_y = self.height - 40;
        let bar_color = Color::rgba(30, 30, 50, 200);

        display.fill_rect(0, bar_y, self.width, 40, bar_color);

        let indicator_x = self.width / 2 - 10;
        let indicator_y = bar_y + 10;

        if self.player.is_playing() {
            // Pause icon
            display.fill_rect(indicator_x, indicator_y, 8, 20, Color::WHITE);
            display.fill_rect(indicator_x + 12, indicator_y, 8, 20, Color::WHITE);
        } else {
            // Play icon (triangle approximation)
            for i in 0..20u32 {
                let w = i.min(20 - i) + 1;
                display.fill_rect(indicator_x + i, indicator_y + (10 - w / 2), 1, w, Color::WHITE);
            }
        }

        // Prev/Next buttons
        display.fill_rect(self.width / 4, indicator_y + 5, 10, 10, Color::LIGHT_GRAY);
        display.fill_rect(self.width * 3 / 4, indicator_y + 5, 10, 10, Color::LIGHT_GRAY);

        // Animation name indicator
        let name_width = match self.player.current() {
            AnimationType::BouncingBall => 96,
            AnimationType::ColorCycle => 88,
            AnimationType::Spinner => 56,
        };
        display.fill_rect((self.width - name_width) / 2, 10, name_width, 8, Color::LIGHT_GRAY);
    }
}
