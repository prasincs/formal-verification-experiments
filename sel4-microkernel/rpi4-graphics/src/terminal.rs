//! # Simple Graphical Terminal
//!
//! Text-mode terminal rendered on the HDMI framebuffer.
//! Supports basic cursor movement, scrolling, and newline handling.

use crate::font::{draw_char, CHAR_HEIGHT, CHAR_WIDTH};
use crate::framebuffer::Framebuffer;
use crate::graphics::Color;

/// Maximum terminal dimensions (160x90 fits 1280x720 with 8x8 font)
pub const MAX_COLS: usize = 160;
pub const MAX_ROWS: usize = 90;

/// Graphical terminal state
pub struct Terminal {
    /// Character buffer [row][col]
    buffer: [[char; MAX_COLS]; MAX_ROWS],
    /// Actual columns based on screen width
    cols: usize,
    /// Actual rows based on screen height
    rows: usize,
    /// Cursor column (0-indexed)
    cursor_col: usize,
    /// Cursor row (0-indexed)
    cursor_row: usize,
    /// Foreground color
    fg: Color,
    /// Background color
    bg: Color,
}

impl Terminal {
    /// Create a new terminal sized to the framebuffer
    pub fn new(fb: &Framebuffer, fg: Color, bg: Color) -> Self {
        let (width, height) = fb.dimensions();
        let cols = (width / CHAR_WIDTH) as usize;
        let rows = (height / CHAR_HEIGHT) as usize;

        // Clamp to max dimensions
        let cols = cols.min(MAX_COLS);
        let rows = rows.min(MAX_ROWS);

        Self {
            buffer: [[' '; MAX_COLS]; MAX_ROWS],
            cols,
            rows,
            cursor_col: 0,
            cursor_row: 0,
            fg,
            bg,
        }
    }

    /// Get terminal dimensions (cols, rows)
    pub fn dimensions(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    /// Get cursor position (col, row)
    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_col, self.cursor_row)
    }

    /// Set foreground color
    pub fn set_fg(&mut self, color: Color) {
        self.fg = color;
    }

    /// Set background color
    pub fn set_bg(&mut self, color: Color) {
        self.bg = color;
    }

    /// Clear the terminal and reset cursor
    pub fn clear(&mut self, fb: &mut Framebuffer) {
        // Clear buffer
        for row in 0..self.rows {
            for col in 0..self.cols {
                self.buffer[row][col] = ' ';
            }
        }

        // Reset cursor
        self.cursor_col = 0;
        self.cursor_row = 0;

        // Clear screen
        fb.clear(self.bg);
    }

    /// Write a single character at the cursor position
    pub fn putchar(&mut self, fb: &mut Framebuffer, c: char) {
        match c {
            '\n' => {
                self.cursor_col = 0;
                self.cursor_row += 1;
            }
            '\r' => {
                self.cursor_col = 0;
            }
            '\x08' => {
                // Backspace
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                    self.buffer[self.cursor_row][self.cursor_col] = ' ';
                    self.draw_char_at(fb, self.cursor_col, self.cursor_row);
                }
            }
            _ => {
                // Store in buffer
                self.buffer[self.cursor_row][self.cursor_col] = c;

                // Draw the character
                self.draw_char_at(fb, self.cursor_col, self.cursor_row);

                // Advance cursor
                self.cursor_col += 1;

                // Line wrap
                if self.cursor_col >= self.cols {
                    self.cursor_col = 0;
                    self.cursor_row += 1;
                }
            }
        }

        // Scroll if needed
        if self.cursor_row >= self.rows {
            self.scroll(fb);
            self.cursor_row = self.rows - 1;
        }
    }

    /// Write a string to the terminal
    pub fn print(&mut self, fb: &mut Framebuffer, s: &str) {
        for c in s.chars() {
            self.putchar(fb, c);
        }
    }

    /// Print with newline
    pub fn println(&mut self, fb: &mut Framebuffer, s: &str) {
        self.print(fb, s);
        self.putchar(fb, '\n');
    }

    /// Move cursor to position (col, row)
    pub fn move_to(&mut self, col: usize, row: usize) {
        self.cursor_col = col.min(self.cols - 1);
        self.cursor_row = row.min(self.rows - 1);
    }

    /// Scroll the terminal up by one line
    fn scroll(&mut self, fb: &mut Framebuffer) {
        // Shift buffer up
        for row in 1..self.rows {
            for col in 0..self.cols {
                self.buffer[row - 1][col] = self.buffer[row][col];
            }
        }

        // Clear last row
        for col in 0..self.cols {
            self.buffer[self.rows - 1][col] = ' ';
        }

        // Redraw entire screen
        self.redraw(fb);
    }

    /// Draw a single character at buffer position
    fn draw_char_at(&self, fb: &mut Framebuffer, col: usize, row: usize) {
        let x = (col as u32) * CHAR_WIDTH;
        let y = (row as u32) * CHAR_HEIGHT;
        let c = self.buffer[row][col];

        // Draw background
        fb.fill_rect(x, y, CHAR_WIDTH, CHAR_HEIGHT, self.bg);

        // Draw character
        draw_char(fb, x, y, c, self.fg);
    }

    /// Redraw the entire terminal
    pub fn redraw(&self, fb: &mut Framebuffer) {
        fb.clear(self.bg);

        for row in 0..self.rows {
            for col in 0..self.cols {
                let c = self.buffer[row][col];
                if c != ' ' {
                    let x = (col as u32) * CHAR_WIDTH;
                    let y = (row as u32) * CHAR_HEIGHT;
                    draw_char(fb, x, y, c, self.fg);
                }
            }
        }
    }
}

/// Common terminal colors
impl Color {
    pub const TERM_BLACK: Color = Color::rgb(0, 0, 0);
    pub const TERM_WHITE: Color = Color::rgb(255, 255, 255);
    pub const TERM_GREEN: Color = Color::rgb(0, 255, 0);
    pub const TERM_AMBER: Color = Color::rgb(255, 176, 0);
    pub const TERM_CYAN: Color = Color::rgb(0, 255, 255);
}
