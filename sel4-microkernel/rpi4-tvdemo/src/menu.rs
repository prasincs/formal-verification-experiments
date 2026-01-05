//! Menu system for TV demo
//!
//! Provides a navigable menu with highlight selection.

use crate::backend::{DisplayBackend, Color};

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
    pub bg_color: Color,
    /// Text color (normal)
    pub text_color: Color,
    /// Highlight background color
    pub highlight_bg: Color,
    /// Highlight text color
    pub highlight_text: Color,
    /// Disabled text color
    pub disabled_color: Color,
    /// Item height in pixels
    pub item_height: u32,
    /// Left padding
    pub padding_left: u32,
    /// Top padding
    pub padding_top: u32,
}

impl Default for MenuStyle {
    fn default() -> Self {
        Self::dark()
    }
}

impl MenuStyle {
    /// Create a dark theme style
    pub const fn dark() -> Self {
        Self {
            bg_color: Color::rgb(20, 20, 30),
            text_color: Color::WHITE,
            highlight_bg: Color::rgb(60, 120, 200),
            highlight_text: Color::WHITE,
            disabled_color: Color::rgb(100, 100, 100),
            item_height: 32,
            padding_left: 20,
            padding_top: 40,
        }
    }

    /// Create a light theme style
    pub const fn light() -> Self {
        Self {
            bg_color: Color::WHITE,
            text_color: Color::BLACK,
            highlight_bg: Color::rgb(100, 150, 255),
            highlight_text: Color::BLACK,
            disabled_color: Color::rgb(180, 180, 180),
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
    /// Screen dimensions
    width: u32,
    height: u32,
}

impl Menu {
    /// Create a new empty menu
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            items: [MenuItem::new(0); MAX_MENU_ITEMS],
            item_count: 0,
            selected: 0,
            style: MenuStyle::dark(),
            title: [0; 32],
            title_len: 0,
            width,
            height,
        }
    }

    /// Create a menu with a title
    pub fn with_title(width: u32, height: u32, title: &str) -> Self {
        let mut menu = Self::new(width, height);
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

        let mut new_sel = self.selected;
        loop {
            new_sel = (new_sel + 1) % self.item_count;

            if self.items[new_sel].enabled || new_sel == self.selected {
                break;
            }
        }
        self.selected = new_sel;
    }

    /// Render the menu to a display
    pub fn render<D: DisplayBackend>(&self, display: &mut D) {
        let style = &self.style;

        // Clear background
        display.clear(style.bg_color);

        // Draw title bar (if present)
        if self.title_len > 0 {
            display.fill_rect(0, 0, self.width, 30, Color::rgb(40, 40, 60));
        }

        // Draw menu items
        for (i, item) in self.items[..self.item_count].iter().enumerate() {
            let y = style.padding_top + (i as u32) * style.item_height;
            let is_selected = i == self.selected;

            // Background
            let bg = if is_selected {
                style.highlight_bg
            } else {
                style.bg_color
            };
            display.fill_rect(0, y, self.width, style.item_height, bg);

            // Selection indicator
            if is_selected {
                display.fill_rect(5, y + 10, 8, 12, style.highlight_text);
            }

            // Text placeholder (actual text rendering would require a font)
            let text_color = if !item.enabled {
                style.disabled_color
            } else if is_selected {
                style.highlight_text
            } else {
                style.text_color
            };

            let text_y = y + (style.item_height - 8) / 2;
            let text_width = (item.label_len as u32) * 8;
            display.fill_rect(
                style.padding_left,
                text_y,
                text_width.min(self.width - style.padding_left - 10),
                8,
                text_color,
            );
        }
    }

    /// Get item count
    pub fn item_count(&self) -> usize {
        self.item_count
    }
}
