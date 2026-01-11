//! Menu system for TV demo
//!
//! Provides a navigable menu with highlight selection.

use crate::display::{Framebuffer, Rgb565};

/// Maximum number of menu items
pub const MAX_MENU_ITEMS: usize = 10;

/// Menu item with label and optional icon
#[derive(Clone, Copy)]
pub struct MenuItem {
    /// Display label (null-terminated, max 32 chars)
    label: [u8; 32],
    label_len: usize,
    /// Whether this item is enabled
    pub enabled: bool,
    /// Item identifier for action handling
    pub id: u8,
}

impl MenuItem {
    /// Create a new menu item
    pub const fn new(id: u8) -> Self {
        Self {
            label: [0; 32],
            label_len: 0,
            enabled: true,
            id,
        }
    }

    /// Create a menu item with a label
    pub fn with_label(id: u8, label: &str) -> Self {
        let mut item = Self::new(id);
        item.set_label(label);
        item
    }

    /// Set the label text
    pub fn set_label(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let len = bytes.len().min(31);
        self.label[..len].copy_from_slice(&bytes[..len]);
        self.label_len = len;
    }

    /// Get the label as a string slice
    pub fn label(&self) -> &str {
        core::str::from_utf8(&self.label[..self.label_len]).unwrap_or("")
    }
}

/// Menu styling options
#[derive(Clone, Copy)]
pub struct MenuStyle {
    /// Background color
    pub bg_color: Rgb565,
    /// Text color (normal)
    pub text_color: Rgb565,
    /// Highlight background color
    pub highlight_bg: Rgb565,
    /// Highlight text color
    pub highlight_text: Rgb565,
    /// Disabled text color
    pub disabled_color: Rgb565,
    /// Item height in pixels
    pub item_height: u16,
    /// Left padding
    pub padding_left: u16,
    /// Top padding
    pub padding_top: u16,
}

impl Default for MenuStyle {
    fn default() -> Self {
        Self {
            bg_color: Rgb565::from_rgb(20, 20, 30),        // Dark blue-gray
            text_color: Rgb565::WHITE,
            highlight_bg: Rgb565::from_rgb(60, 120, 200),  // Blue highlight
            highlight_text: Rgb565::WHITE,
            disabled_color: Rgb565::from_rgb(100, 100, 100),
            item_height: 32,
            padding_left: 20,
            padding_top: 40,
        }
    }
}

impl MenuStyle {
    /// Create a dark theme style
    pub const fn dark() -> Self {
        Self {
            bg_color: Rgb565(0x0000),
            text_color: Rgb565(0xFFFF),
            highlight_bg: Rgb565::from_rgb(40, 80, 160),
            highlight_text: Rgb565(0xFFFF),
            disabled_color: Rgb565::from_rgb(80, 80, 80),
            item_height: 32,
            padding_left: 20,
            padding_top: 40,
        }
    }

    /// Create a light theme style
    pub const fn light() -> Self {
        Self {
            bg_color: Rgb565(0xFFFF),
            text_color: Rgb565(0x0000),
            highlight_bg: Rgb565::from_rgb(100, 150, 255),
            highlight_text: Rgb565(0x0000),
            disabled_color: Rgb565::from_rgb(180, 180, 180),
            item_height: 32,
            padding_left: 20,
            padding_top: 40,
        }
    }
}

/// Navigable menu
pub struct Menu {
    items: [MenuItem; MAX_MENU_ITEMS],
    item_count: usize,
    selected: usize,
    style: MenuStyle,
    /// Title of the menu
    title: [u8; 32],
    title_len: usize,
}

impl Menu {
    /// Create a new empty menu
    pub const fn new() -> Self {
        Self {
            items: [MenuItem::new(0); MAX_MENU_ITEMS],
            item_count: 0,
            selected: 0,
            style: MenuStyle::dark(),
            title: [0; 32],
            title_len: 0,
        }
    }

    /// Create a menu with a title
    pub fn with_title(title: &str) -> Self {
        let mut menu = Self::new();
        menu.set_title(title);
        menu
    }

    /// Set the menu title
    pub fn set_title(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let len = bytes.len().min(31);
        self.title[..len].copy_from_slice(&bytes[..len]);
        self.title_len = len;
    }

    /// Get the menu title
    pub fn title(&self) -> &str {
        core::str::from_utf8(&self.title[..self.title_len]).unwrap_or("")
    }

    /// Add an item to the menu
    pub fn add_item(&mut self, item: MenuItem) -> bool {
        if self.item_count < MAX_MENU_ITEMS {
            self.items[self.item_count] = item;
            self.item_count += 1;
            true
        } else {
            false
        }
    }

    /// Clear all items
    pub fn clear(&mut self) {
        self.item_count = 0;
        self.selected = 0;
    }

    /// Set the menu style
    pub fn set_style(&mut self, style: MenuStyle) {
        self.style = style;
    }

    /// Get the currently selected item
    pub fn selected_item(&self) -> Option<&MenuItem> {
        if self.selected < self.item_count {
            Some(&self.items[self.selected])
        } else {
            None
        }
    }

    /// Get the selected index
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// Move selection up
    pub fn move_up(&mut self) {
        if self.item_count == 0 {
            return;
        }

        // Find previous enabled item
        let mut new_sel = self.selected;
        loop {
            if new_sel == 0 {
                new_sel = self.item_count - 1;
            } else {
                new_sel -= 1;
            }

            if self.items[new_sel].enabled || new_sel == self.selected {
                break;
            }
        }
        self.selected = new_sel;
    }

    /// Move selection down
    pub fn move_down(&mut self) {
        if self.item_count == 0 {
            return;
        }

        // Find next enabled item
        let mut new_sel = self.selected;
        loop {
            new_sel = (new_sel + 1) % self.item_count;

            if self.items[new_sel].enabled || new_sel == self.selected {
                break;
            }
        }
        self.selected = new_sel;
    }

    /// Render the menu to a framebuffer
    pub fn render(&self, fb: &mut Framebuffer) {
        let style = &self.style;

        // Clear background
        fb.clear(style.bg_color);

        // Draw title (if present)
        if self.title_len > 0 {
            // Draw title bar
            fb.fill_rect(0, 0, 320, 30, Rgb565::from_rgb(40, 40, 60));
            // Title text would be drawn here with a font renderer
            // For now, we just have the title bar
        }

        // Draw menu items
        for (i, item) in self.items[..self.item_count].iter().enumerate() {
            let y = style.padding_top + (i as u16) * style.item_height;
            let is_selected = i == self.selected;

            // Background
            let bg = if is_selected {
                style.highlight_bg
            } else {
                style.bg_color
            };
            fb.fill_rect(0, y, 320, style.item_height, bg);

            // Selection indicator
            if is_selected {
                // Draw left arrow/cursor
                fb.fill_rect(5, y + 10, 8, 12, style.highlight_text);
            }

            // Text color depends on enabled state
            let _text_color = if !item.enabled {
                style.disabled_color
            } else if is_selected {
                style.highlight_text
            } else {
                style.text_color
            };

            // Actual text rendering would require a font system
            // For now we draw a placeholder bar representing text
            let text_y = y + (style.item_height - 8) / 2;
            let text_width = (item.label_len as u16) * 8;
            fb.fill_rect(
                style.padding_left,
                text_y,
                text_width.min(280),
                8,
                if is_selected { style.highlight_text } else { style.text_color },
            );
        }
    }

    /// Get item count
    pub fn item_count(&self) -> usize {
        self.item_count
    }
}

impl Default for Menu {
    fn default() -> Self {
        Self::new()
    }
}
