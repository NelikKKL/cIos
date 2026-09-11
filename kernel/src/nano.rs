//! Клон GNU nano поверх собственных примитивов cIos (fs::, framebuffer
//! через terminal.rs). Это НЕ порт исходников nano.c — тот зависит от
//! ncurses/termios, POSIX-файлов, fork/exec (spellcheck/linter) и
//! regex.h, которых в no_std/freestanding ядре нет и быть не может.
//! Вместо этого здесь заново на Rust реализовано поведение и раскладка
//! экрана, максимально близко к оригиналу (title bar, статус-строка,
//! нижняя панель горячих клавиш — см. terminal.rs draw_nano*).
//!
//! Сознательно НЕ реализовано (см. NANO.md в корне репозитория за
//! полным списком причин):
//!   - ^T spellcheck/linter — требуют внешнего процесса (aspell и т.п.),
//!     а в cIos вообще нет процессов/fork.
//!   - Переключение в regex-режим поиска (M-R) — нет regex-движка.
//!     Обычный (не-regex) поиск в самом nano и так не требует regex —
//!     см. ISSET(USE_REGEXP) в search.c оригинала — поэтому он здесь есть.
//!   - Undo/redo (M-U/M-E) и мультибуферы (M-F, M-</M->).
//!   - Несколько более редких переключателей (mouse, suspend, softwrap,
//!     DOS/Mac line endings, backup-файлы, execute-command) — можно
//!     добавить отдельно, не являются пока частью этого клона.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use pc_keyboard::{DecodedKey, KeyCode};

use crate::fs;

fn is_printable(c: char) -> bool {
    (c as u32) >= 0x20 && (c as u32) < 0x7F
}

fn byte_idx(line: &str, col: usize) -> usize {
    match line.char_indices().nth(col) {
        Some((i, _)) => i,
        None => line.len(),
    }
}

fn char_len(line: &str) -> usize {
    line.chars().count()
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Модальные подсказки внизу экрана — аналог prompt-режимов nano
/// (statusbar() с последующим вводом одной строки).
pub enum Prompt {
    Help,
    SaveAs { input: String },
    ExitConfirm,
    Search { input: String },
    ReplaceFind { input: String },
    ReplaceWith { find: String, input: String },
    Goto { input: String },
    InsertFile { input: String },
}

pub enum Outcome {
    Continue,
    /// Выход из режима nano обратно в шелл, с сообщением для истории.
    Exit(String),
}

/// Список ярлыков для нижней панели — то же самое семантически, что
/// bottombars()/onekey() в winio.c оригинала, но только для функций,
/// которые здесь реально реализованы (показывать шорткат для того,
/// чего нет, было бы враньём, а не "точным клоном").
pub const MAIN_SHORTCUTS: &[(&str, &str)] = &[
    ("^G", "Get Help"),
    ("^X", "Exit"),
    ("^O", "Write Out"),
    ("^J", "Justify"),
    ("^R", "Read File"),
    ("^W", "Where Is"),
    ("^\\", "Replace"),
    ("^_", "Go To Line"),
    ("^K", "Cut Text"),
    ("^U", "Paste Text"),
    ("^C", "Cur Pos"),
    ("^^", "Mark Text"),
];

pub const HELP_LINES: &[&str] = &[
    "cIos nano-clone -- quick reference",
    "",
    "^G  Get Help        ^X  Exit             ^O  Write Out",
    "^R  Read File        ^W  Where Is (search) ^\\  Replace",
    "^K  Cut line/region   ^U  Paste             ^^  Mark text (M-A)",
    "^_  Go To Line        ^J  Justify paragraph ^C  Show cursor position",
    "^A/Home Line start     ^E/End Line end      ^B/^F/arrows Move",
    "^P/^N/arrows  Up/Down  ^Y/PgUp  Page up     ^V/PgDn  Page down",
    "^Left/^Right   Word jump                    M-} / M-{  Indent/Unindent",
    "^L  Refresh screen     Tab  Insert tab       Del/^D  Delete forward",
    "",
    "Not implemented (see NANO.md): spellcheck/linter (^T), regex search,",
    "undo/redo, multiple buffers.",
    "",
    "Press any key to close this help.",
];

pub struct Editor {
    pub lines: Vec<String>,
    pub filename: Option<String>,
    pub modified: bool,
    pub cursor_line: usize,
    pub cursor_col: usize,
    /// "Желаемая" колонка при движении вверх/вниз (как placewewant в
    /// оригинале) — чтобы проходя короткую строку не терять исходную
    /// горизонтальную позицию на следующей длинной строке.
    want_col: usize,
    pub mark: Option<(usize, usize)>,
    cutbuffer: Vec<String>,
    last_action_was_cut: bool,
    last_search: Option<String>,
    pending_exit_after_save: bool,
    pub status: String,
    pub prompt: Option<Prompt>,
}

impl Editor {
    /// Открывает файл по абсолютному пути (уже resolve()-нутому), либо
    /// заводит пустой буфер ("New Buffer"), если path пуст или файла
    /// не существует ещё — как `nano newfile.txt` в оригинале.
    pub fn open(path: Option<&str>) -> Editor {
        let (lines, filename) = match path {
            Some(p) => match fs::read(p) {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
                    if lines.is_empty() {
                        lines.push(String::new());
                    }
                    (lines, Some(p.to_string()))
                }
                Err(_) => (alloc::vec![String::new()], Some(p.to_string())),
            },
            None => (alloc::vec![String::new()], None),
        };

        Editor {
            lines,
            filename,
            modified: false,
            cursor_line: 0,
            cursor_col: 0,
            want_col: 0,
            mark: None,
            cutbuffer: Vec::new(),
            last_action_was_cut: false,
            last_search: None,
            pending_exit_after_save: false,
            status: String::from("Welcome to cIos nano-clone. ^G for help."),
            prompt: None,
        }
    }

    pub fn is_prompting(&self) -> bool {
        self.prompt.is_some()
    }

    // ---------------------------------------------------------------
    // Точка входа: одно событие клавиатуры за раз.
    // ---------------------------------------------------------------
    pub fn handle_key(&mut self, key: DecodedKey, ctrl: bool, alt: bool) -> Outcome {
        if let Some(prompt) = self.prompt.take() {
            return self.handle_prompt_key(prompt, key, ctrl);
        }
        self.handle_normal_key(key, ctrl, alt)
    }

    fn handle_normal_key(&mut self, key: DecodedKey, ctrl: bool, alt: bool) -> Outcome {
        // Ctrl+буква -- подавляющее большинство команд nano.
        if ctrl {
            if let DecodedKey::Unicode(raw) = key {
                let c = raw.to_ascii_lowercase();
                match c {
                    'g' => {
                        self.prompt = Some(Prompt::Help);
                        return Outcome::Continue;
                    }
                    'x' => return self.begin_exit(),
                    'o' => {
                        let input = self.filename.clone().unwrap_or_default();
                        self.prompt = Some(Prompt::SaveAs { input });
                        return Outcome::Continue;
                    }
                    'r' => {
                        self.prompt = Some(Prompt::InsertFile { input: String::new() });
                        return Outcome::Continue;
                    }
                    'w' => {
                        self.prompt = Some(Prompt::Search { input: String::new() });
                        return Outcome::Continue;
                    }
                    '\\' => {
                        self.prompt = Some(Prompt::ReplaceFind { input: String::new() });
                        return Outcome::Continue;
                    }
                    'k' => {
                        self.cut();
                        return Outcome::Continue;
                    }
                    'u' => {
                        self.uncut();
                        return Outcome::Continue;
                    }
                    '_' => {
                        self.prompt = Some(Prompt::Goto { input: String::new() });
                        return Outcome::Continue;
                    }
                    'j' => {
                        self.justify();
                        return Outcome::Continue;
                    }
                    'c' => {
                        self.report_cursor_pos();
                        return Outcome::Continue;
                    }
                    'l' => {
                        self.status = String::from("Refreshed");
                        return Outcome::Continue;
                    }
                    '^' => {
                        self.toggle_mark();
                        return Outcome::Continue;
                    }
                    'a' => {
                        self.move_home();
                        return Outcome::Continue;
                    }
                    'e' => {
                        self.move_end();
                        return Outcome::Continue;
                    }
                    'p' => {
                        self.move_up();
                        return Outcome::Continue;
                    }
                    'n' => {
                        self.move_down();
                        return Outcome::Continue;
                    }
                    'b' => {
                        self.move_left();
                        return Outcome::Continue;
                    }
                    'f' => {
                        self.move_right();
                        return Outcome::Continue;
                    }
                    'y' => {
                        self.page_up();
                        return Outcome::Continue;
                    }
                    'v' => {
                        self.page_down();
                        return Outcome::Continue;
                    }
                    'd' => {
                        self.delete_forward();
                        return Outcome::Continue;
                    }
                    'h' => {
                        self.backspace();
                        return Outcome::Continue;
                    }
                    'i' => {
                        self.insert_char('\t');
                        return Outcome::Continue;
                    }
                    'm' => {
                        self.newline();
                        return Outcome::Continue;
                    }
                    '6' => {
                        self.toggle_mark();
                        return Outcome::Continue;
                    }
                    _ => {}
                }
                // Необработанная Ctrl+буква не должна допечататься как
                // обычный символ (то есть Ctrl+Q не должен вставить
                // 'q') — просто игнорируем её, как непривязанный
                // шорткат в оригинале.
                return Outcome::Continue;
            }
            // Ctrl+стрелка -- прыжок по словам.
            if let DecodedKey::RawKey(code) = key {
                match code {
                    KeyCode::ArrowLeft => {
                        self.move_word_left();
                        return Outcome::Continue;
                    }
                    KeyCode::ArrowRight => {
                        self.move_word_right();
                        return Outcome::Continue;
                    }
                    _ => {}
                }
            }
        }

        if alt {
            if let DecodedKey::Unicode(raw) = key {
                match raw {
                    'w' | 'W' => {
                        self.research();
                        return Outcome::Continue;
                    }
                    'a' | 'A' => {
                        self.toggle_mark();
                        return Outcome::Continue;
                    }
                    '}' => {
                        self.indent();
                        return Outcome::Continue;
                    }
                    '{' => {
                        self.unindent();
                        return Outcome::Continue;
                    }
                    _ => {}
                }
                // Как и с Ctrl выше: непривязанный Alt-шорткат не
                // должен допечататься как обычный символ.
                return Outcome::Continue;
            }
        }

        match key {
            DecodedKey::Unicode('\n') | DecodedKey::Unicode('\r') => self.newline(),
            DecodedKey::Unicode('\u{8}') => self.backspace(),
            DecodedKey::Unicode('\t') => self.insert_char('\t'),
            DecodedKey::RawKey(KeyCode::Backspace) => self.backspace(),
            DecodedKey::RawKey(KeyCode::Delete) => self.delete_forward(),
            DecodedKey::RawKey(KeyCode::ArrowLeft) => self.move_left(),
            DecodedKey::RawKey(KeyCode::ArrowRight) => self.move_right(),
            DecodedKey::RawKey(KeyCode::ArrowUp) => self.move_up(),
            DecodedKey::RawKey(KeyCode::ArrowDown) => self.move_down(),
            DecodedKey::RawKey(KeyCode::Home) => self.move_home(),
            DecodedKey::RawKey(KeyCode::End) => self.move_end(),
            DecodedKey::RawKey(KeyCode::PageUp) => self.page_up(),
            DecodedKey::RawKey(KeyCode::PageDown) => self.page_down(),
            DecodedKey::RawKey(KeyCode::Escape) => {
                if self.mark.is_some() {
                    self.mark = None;
                    self.status = String::from("Mark Unset");
                }
            }
            DecodedKey::Unicode(c) if is_printable(c) => self.insert_char(c),
            _ => {}
        }

        Outcome::Continue
    }

    fn begin_exit(&mut self) -> Outcome {
        if self.modified {
            self.prompt = Some(Prompt::ExitConfirm);
            Outcome::Continue
        } else {
            Outcome::Exit(String::from("nano: closed"))
        }
    }

    // ---------------------------------------------------------------
    // Ввод в модальных подсказках (SaveAs/Search/Goto/...).
    // ---------------------------------------------------------------
    fn handle_prompt_key(&mut self, prompt: Prompt, key: DecodedKey, ctrl: bool) -> Outcome {
        let is_enter = matches!(key, DecodedKey::Unicode('\n') | DecodedKey::Unicode('\r'));
        let is_cancel = matches!(key, DecodedKey::RawKey(KeyCode::Escape))
            || (ctrl && matches!(key, DecodedKey::Unicode('c') | DecodedKey::Unicode('C')));
        let is_backspace = matches!(
            key,
            DecodedKey::Unicode('\u{8}') | DecodedKey::RawKey(KeyCode::Backspace)
        );

        match prompt {
            Prompt::Help => {
                // Любая клавиша закрывает справку.
                self.prompt = None;
            }
            Prompt::ExitConfirm => match key {
                DecodedKey::Unicode('y') | DecodedKey::Unicode('Y') => {
                    if let Some(name) = self.filename.clone() {
                        self.write_to(&name);
                        return Outcome::Exit(format!("nano: wrote and closed '{name}'"));
                    } else {
                        self.pending_exit_after_save = true;
                        self.prompt = Some(Prompt::SaveAs { input: String::new() });
                    }
                }
                DecodedKey::Unicode('n') | DecodedKey::Unicode('N') => {
                    return Outcome::Exit(String::from("nano: closed without saving"));
                }
                _ if is_cancel => {
                    self.status = String::from("Cancelled");
                }
                _ => {
                    self.prompt = Some(Prompt::ExitConfirm);
                }
            },
            Prompt::SaveAs { mut input } => {
                if is_enter {
                    let name = if input.is_empty() {
                        self.filename.clone().unwrap_or_default()
                    } else {
                        input.clone()
                    };
                    if name.is_empty() {
                        self.status = String::from("Cancelled: no file name");
                        self.pending_exit_after_save = false;
                    } else {
                        self.write_to(&name);
                        if self.pending_exit_after_save {
                            self.pending_exit_after_save = false;
                            return Outcome::Exit(format!("nano: wrote and closed '{name}'"));
                        }
                    }
                } else if is_cancel {
                    self.status = String::from("Cancelled");
                    self.pending_exit_after_save = false;
                } else if is_backspace {
                    input.pop();
                    self.prompt = Some(Prompt::SaveAs { input });
                } else if let DecodedKey::Unicode(c) = key {
                    if is_printable(c) {
                        input.push(c);
                    }
                    self.prompt = Some(Prompt::SaveAs { input });
                } else {
                    self.prompt = Some(Prompt::SaveAs { input });
                }
            }
            Prompt::Search { mut input } => {
                if is_enter {
                    let needle = if input.is_empty() {
                        self.last_search.clone().unwrap_or_default()
                    } else {
                        self.last_search = Some(input.clone());
                        input.clone()
                    };
                    if !needle.is_empty() {
                        self.do_search(&needle);
                    }
                } else if is_cancel {
                    self.status = String::from("Cancelled");
                } else if is_backspace {
                    input.pop();
                    self.prompt = Some(Prompt::Search { input });
                } else if let DecodedKey::Unicode(c) = key {
                    if is_printable(c) {
                        input.push(c);
                    }
                    self.prompt = Some(Prompt::Search { input });
                } else {
                    self.prompt = Some(Prompt::Search { input });
                }
            }
            Prompt::ReplaceFind { mut input } => {
                if is_enter {
                    if !input.is_empty() {
                        self.prompt = Some(Prompt::ReplaceWith { find: input, input: String::new() });
                    } else {
                        self.status = String::from("Cancelled");
                    }
                } else if is_cancel {
                    self.status = String::from("Cancelled");
                } else if is_backspace {
                    input.pop();
                    self.prompt = Some(Prompt::ReplaceFind { input });
                } else if let DecodedKey::Unicode(c) = key {
                    if is_printable(c) {
                        input.push(c);
                    }
                    self.prompt = Some(Prompt::ReplaceFind { input });
                } else {
                    self.prompt = Some(Prompt::ReplaceFind { input });
                }
            }
            Prompt::ReplaceWith { find, mut input } => {
                if is_enter {
                    let n = self.replace_all(&find, &input);
                    self.status = format!("Replaced {n} occurrence(s)");
                } else if is_cancel {
                    self.status = String::from("Cancelled");
                } else if is_backspace {
                    input.pop();
                    self.prompt = Some(Prompt::ReplaceWith { find, input });
                } else if let DecodedKey::Unicode(c) = key {
                    if is_printable(c) {
                        input.push(c);
                    }
                    self.prompt = Some(Prompt::ReplaceWith { find, input });
                } else {
                    self.prompt = Some(Prompt::ReplaceWith { find, input });
                }
            }
            Prompt::Goto { mut input } => {
                if is_enter {
                    self.goto(&input);
                } else if is_cancel {
                    self.status = String::from("Cancelled");
                } else if is_backspace {
                    input.pop();
                    self.prompt = Some(Prompt::Goto { input });
                } else if let DecodedKey::Unicode(c) = key {
                    if c.is_ascii_digit() || c == ',' {
                        input.push(c);
                    }
                    self.prompt = Some(Prompt::Goto { input });
                } else {
                    self.prompt = Some(Prompt::Goto { input });
                }
            }
            Prompt::InsertFile { mut input } => {
                if is_enter {
                    self.insert_file(&input);
                } else if is_cancel {
                    self.status = String::from("Cancelled");
                } else if is_backspace {
                    input.pop();
                    self.prompt = Some(Prompt::InsertFile { input });
                } else if let DecodedKey::Unicode(c) = key {
                    if is_printable(c) {
                        input.push(c);
                    }
                    self.prompt = Some(Prompt::InsertFile { input });
                } else {
                    self.prompt = Some(Prompt::InsertFile { input });
                }
            }
        }

        Outcome::Continue
    }

    // ---------------------------------------------------------------
    // Движение курсора
    // ---------------------------------------------------------------
    fn clamp_col(&mut self) {
        let len = char_len(&self.lines[self.cursor_line]);
        if self.cursor_col > len {
            self.cursor_col = len;
        }
    }

    fn move_left(&mut self) {
        self.last_action_was_cut = false;
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = char_len(&self.lines[self.cursor_line]);
        }
        self.want_col = self.cursor_col;
    }

    fn move_right(&mut self) {
        self.last_action_was_cut = false;
        let len = char_len(&self.lines[self.cursor_line]);
        if self.cursor_col < len {
            self.cursor_col += 1;
        } else if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            self.cursor_col = 0;
        }
        self.want_col = self.cursor_col;
    }

    fn move_up(&mut self) {
        self.last_action_was_cut = false;
        if self.cursor_line > 0 {
            self.cursor_line -= 1;
            let len = char_len(&self.lines[self.cursor_line]);
            self.cursor_col = self.want_col.min(len);
        }
    }

    fn move_down(&mut self) {
        self.last_action_was_cut = false;
        if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            let len = char_len(&self.lines[self.cursor_line]);
            self.cursor_col = self.want_col.min(len);
        }
    }

    fn move_home(&mut self) {
        self.last_action_was_cut = false;
        self.cursor_col = 0;
        self.want_col = 0;
    }

    fn move_end(&mut self) {
        self.last_action_was_cut = false;
        self.cursor_col = char_len(&self.lines[self.cursor_line]);
        self.want_col = self.cursor_col;
    }

    fn page_up(&mut self) {
        self.last_action_was_cut = false;
        const PAGE: usize = 16;
        self.cursor_line = self.cursor_line.saturating_sub(PAGE);
        self.clamp_col();
    }

    fn page_down(&mut self) {
        self.last_action_was_cut = false;
        const PAGE: usize = 16;
        self.cursor_line = (self.cursor_line + PAGE).min(self.lines.len() - 1);
        self.clamp_col();
    }

    fn move_word_left(&mut self) {
        self.last_action_was_cut = false;
        if self.cursor_col == 0 {
            if self.cursor_line == 0 {
                return;
            }
            self.cursor_line -= 1;
            self.cursor_col = char_len(&self.lines[self.cursor_line]);
            self.want_col = self.cursor_col;
            return;
        }
        let chars: Vec<char> = self.lines[self.cursor_line].chars().collect();
        let mut i = self.cursor_col;
        while i > 0 && !is_word_char(chars[i - 1]) {
            i -= 1;
        }
        while i > 0 && is_word_char(chars[i - 1]) {
            i -= 1;
        }
        self.cursor_col = i;
        self.want_col = i;
    }

    fn move_word_right(&mut self) {
        self.last_action_was_cut = false;
        let len = char_len(&self.lines[self.cursor_line]);
        if self.cursor_col >= len {
            if self.cursor_line + 1 >= self.lines.len() {
                return;
            }
            self.cursor_line += 1;
            self.cursor_col = 0;
            self.want_col = 0;
            return;
        }
        let chars: Vec<char> = self.lines[self.cursor_line].chars().collect();
        let mut i = self.cursor_col;
        while i < len && is_word_char(chars[i]) {
            i += 1;
        }
        while i < len && !is_word_char(chars[i]) {
            i += 1;
        }
        self.cursor_col = i;
        self.want_col = i;
    }

    // ---------------------------------------------------------------
    // Редактирование текста
    // ---------------------------------------------------------------
    fn insert_char(&mut self, c: char) {
        self.last_action_was_cut = false;
        let line = &mut self.lines[self.cursor_line];
        let b = byte_idx(line, self.cursor_col);
        line.insert(b, c);
        self.cursor_col += 1;
        self.want_col = self.cursor_col;
        self.modified = true;
    }

    fn newline(&mut self) {
        self.last_action_was_cut = false;
        let line = self.lines[self.cursor_line].clone();
        let b = byte_idx(&line, self.cursor_col);
        let (head, tail) = (line[..b].to_string(), line[b..].to_string());
        self.lines[self.cursor_line] = head;
        self.lines.insert(self.cursor_line + 1, tail);
        self.cursor_line += 1;
        self.cursor_col = 0;
        self.want_col = 0;
        self.modified = true;
    }

    fn backspace(&mut self) {
        self.last_action_was_cut = false;
        if self.cursor_col > 0 {
            let line = &mut self.lines[self.cursor_line];
            let b_before = byte_idx(line, self.cursor_col - 1);
            let b_at = byte_idx(line, self.cursor_col);
            line.replace_range(b_before..b_at, "");
            self.cursor_col -= 1;
            self.modified = true;
        } else if self.cursor_line > 0 {
            let current = self.lines.remove(self.cursor_line);
            let prev_len = char_len(&self.lines[self.cursor_line - 1]);
            self.lines[self.cursor_line - 1].push_str(&current);
            self.cursor_line -= 1;
            self.cursor_col = prev_len;
            self.modified = true;
        }
        self.want_col = self.cursor_col;
    }

    fn delete_forward(&mut self) {
        self.last_action_was_cut = false;
        let len = char_len(&self.lines[self.cursor_line]);
        if self.cursor_col < len {
            let line = &mut self.lines[self.cursor_line];
            let b_at = byte_idx(line, self.cursor_col);
            let b_after = byte_idx(line, self.cursor_col + 1);
            line.replace_range(b_at..b_after, "");
            self.modified = true;
        } else if self.cursor_line + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_line + 1);
            self.lines[self.cursor_line].push_str(&next);
            self.modified = true;
        }
    }

    fn tab_indent_of(line: &str) -> usize {
        line.chars().take_while(|c| *c == '\t').count()
    }

    fn indent(&mut self) {
        let (start, end) = self.mark_range_lines();
        for i in start..=end {
            self.lines[i].insert(0, '\t');
        }
        if self.cursor_line >= start && self.cursor_line <= end {
            self.cursor_col += 1;
        }
        self.modified = true;
    }

    fn unindent(&mut self) {
        let (start, end) = self.mark_range_lines();
        for i in start..=end {
            if Self::tab_indent_of(&self.lines[i]) > 0 {
                self.lines[i].remove(0);
                if self.cursor_line == i && self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
            }
        }
        self.modified = true;
    }

    fn mark_range_lines(&self) -> (usize, usize) {
        match self.mark {
            Some((ml, _)) => {
                if ml <= self.cursor_line {
                    (ml, self.cursor_line)
                } else {
                    (self.cursor_line, ml)
                }
            }
            None => (self.cursor_line, self.cursor_line),
        }
    }

    // ---------------------------------------------------------------
    // Пометка / вырезать / вставить / вставить-файл
    // ---------------------------------------------------------------
    fn toggle_mark(&mut self) {
        if self.mark.is_some() {
            self.mark = None;
            self.status = String::from("Mark Unset");
        } else {
            self.mark = Some((self.cursor_line, self.cursor_col));
            self.status = String::from("Mark Set");
        }
    }

    fn cut(&mut self) {
        if self.mark.is_some() {
            self.cut_region();
            return;
        }
        let line = self.lines.remove(self.cursor_line);
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        if self.cursor_line >= self.lines.len() {
            self.cursor_line = self.lines.len() - 1;
        }
        self.cursor_col = 0;
        self.want_col = 0;
        if self.last_action_was_cut {
            self.cutbuffer.push(line);
        } else {
            self.cutbuffer = alloc::vec![line];
        }
        self.last_action_was_cut = true;
        self.modified = true;
    }

    fn cut_region(&mut self) {
        let (mark_line, mark_col) = self.mark.take().unwrap();
        let (sl, sc, el, ec) = if (mark_line, mark_col) <= (self.cursor_line, self.cursor_col) {
            (mark_line, mark_col, self.cursor_line, self.cursor_col)
        } else {
            (self.cursor_line, self.cursor_col, mark_line, mark_col)
        };

        if sl == el {
            let line = self.lines[sl].clone();
            let sb = byte_idx(&line, sc);
            let eb = byte_idx(&line, ec);
            self.cutbuffer = alloc::vec![line[sb..eb].to_string()];
            let mut new_line = line[..sb].to_string();
            new_line.push_str(&line[eb..]);
            self.lines[sl] = new_line;
        } else {
            let start_line = self.lines[sl].clone();
            let sb = byte_idx(&start_line, sc);
            let end_line = self.lines[el].clone();
            let eb = byte_idx(&end_line, ec);
            let mut cut: Vec<String> = alloc::vec![start_line[sb..].to_string()];
            for line in &self.lines[(sl + 1)..el] {
                cut.push(line.clone());
            }
            cut.push(end_line[..eb].to_string());
            let merged = format!("{}{}", &start_line[..sb], &end_line[eb..]);
            self.lines.splice(sl..=el, core::iter::once(merged));
            self.cutbuffer = cut;
        }

        self.cursor_line = sl;
        self.cursor_col = sc;
        self.want_col = sc;
        self.last_action_was_cut = false;
        self.modified = true;
    }

    fn uncut(&mut self) {
        if self.cutbuffer.is_empty() {
            self.status = String::from("Nothing to paste");
            return;
        }
        let text = self.cutbuffer.join("\n");
        self.insert_lines_at_cursor(&text);
        self.last_action_was_cut = false;
        self.modified = true;
    }

    /// Вставляет произвольный (возможно многострочный) текст в позицию
    /// курсора, разбивая текущую строку так же, как это делает paste
    /// или ^R (insert file) в оригинале.
    fn insert_lines_at_cursor(&mut self, text: &str) {
        let mut new_lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
        if new_lines.is_empty() {
            return;
        }
        let current = self.lines[self.cursor_line].clone();
        let b = byte_idx(&current, self.cursor_col);
        let head = current[..b].to_string();
        let tail = current[b..].to_string();

        let head_len_chars = char_len(&head);
        let first_new_len_chars = char_len(&new_lines[0]);

        new_lines[0] = format!("{head}{}", new_lines[0]);
        let last_idx = new_lines.len() - 1;
        new_lines[last_idx] = format!("{}{tail}", new_lines[last_idx]);

        self.lines.splice(self.cursor_line..=self.cursor_line, new_lines);

        if last_idx == 0 {
            self.cursor_col = head_len_chars + first_new_len_chars;
        } else {
            self.cursor_line += last_idx;
            self.cursor_col = char_len(&self.lines[self.cursor_line]) - char_len(&tail);
        }
        self.want_col = self.cursor_col;
    }

    fn insert_file(&mut self, path_input: &str) {
        if path_input.is_empty() {
            self.status = String::from("Cancelled");
            return;
        }
        match fs::read(path_input) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                self.insert_lines_at_cursor(&text);
                self.modified = true;
                self.status = format!("Inserted '{path_input}'");
            }
            Err(e) => {
                self.status = format!("Error reading '{path_input}': {e}");
            }
        }
    }

    // ---------------------------------------------------------------
    // Поиск / замена / переход на строку
    // ---------------------------------------------------------------
    fn do_search(&mut self, needle: &str) {
        let needle_lower = needle.to_lowercase();
        let n = self.lines.len();
        // Ищем начиная со следующей позиции после курсора, с
        // переносом в начало буфера (как обычный, не-regex поиск nano
        // — он по умолчанию тоже wrap-around).
        let start_line = self.cursor_line;
        let start_col = self.cursor_col;

        for offset in 0..=n {
            let li = (start_line + offset) % n;
            let line_lower = self.lines[li].to_lowercase();
            let search_from = if offset == 0 { start_col + 1 } else { 0 };
            let from_byte = byte_idx(&line_lower, search_from.min(char_len(&line_lower)));
            if let Some(pos_byte) = line_lower[from_byte..].find(needle_lower.as_str()) {
                let abs_byte = from_byte + pos_byte;
                let col = line_lower[..abs_byte].chars().count();
                self.cursor_line = li;
                self.cursor_col = col;
                self.want_col = col;
                self.status = if offset > 0 && li < start_line {
                    format!("Search Wrapped: found '{needle}'")
                } else {
                    format!("Found '{needle}'")
                };
                return;
            }
            if offset == n {
                break;
            }
        }
        self.status = format!("\"{needle}\" not found");
    }

    fn research(&mut self) {
        match self.last_search.clone() {
            Some(needle) => self.do_search(&needle),
            None => self.status = String::from("No previous search"),
        }
    }

    fn replace_all(&mut self, find: &str, with: &str) -> usize {
        if find.is_empty() {
            return 0;
        }
        let find_lower = find.to_lowercase();
        let mut count = 0usize;
        for line in self.lines.iter_mut() {
            let mut result = String::new();
            let mut rest = line.as_str();
            loop {
                let rest_lower = rest.to_lowercase();
                match rest_lower.find(find_lower.as_str()) {
                    Some(idx) => {
                        result.push_str(&rest[..idx]);
                        result.push_str(with);
                        rest = &rest[idx + find.len()..];
                        count += 1;
                    }
                    None => {
                        result.push_str(rest);
                        break;
                    }
                }
            }
            *line = result;
        }
        if count > 0 {
            self.modified = true;
            self.cursor_col = self.cursor_col.min(char_len(&self.lines[self.cursor_line]));
        }
        count
    }

    fn goto(&mut self, input: &str) {
        let mut parts = input.split(',');
        let line_str = parts.next().unwrap_or("");
        let col_str = parts.next();

        let target_line: usize = match line_str.trim().parse::<usize>() {
            Ok(n) if n >= 1 => n - 1,
            _ => {
                self.status = String::from("Invalid line number");
                return;
            }
        };
        if target_line >= self.lines.len() {
            self.status = String::from("Line number out of range");
            return;
        }
        self.cursor_line = target_line;

        if let Some(cs) = col_str {
            match cs.trim().parse::<usize>() {
                Ok(n) if n >= 1 => self.cursor_col = (n - 1).min(char_len(&self.lines[target_line])),
                _ => self.cursor_col = 0,
            }
        } else {
            self.cursor_col = 0;
        }
        self.want_col = self.cursor_col;
        self.status = format!("Jumped to line {}", target_line + 1);
    }

    fn report_cursor_pos(&mut self) {
        let total = self.lines.len();
        let chars_before: usize = self.lines[..self.cursor_line].iter().map(|l| char_len(l) + 1).sum();
        let chars_before = chars_before + self.cursor_col;
        let total_chars: usize = self.lines.iter().map(|l| char_len(l) + 1).sum::<usize>().saturating_sub(1);
        self.status = format!(
            "line {}/{} ({}%), col {}, char {}/{} ({}%)",
            self.cursor_line + 1,
            total,
            (self.cursor_line + 1) * 100 / total.max(1),
            self.cursor_col + 1,
            chars_before + 1,
            total_chars + 1,
            (chars_before + 1) * 100 / (total_chars + 1).max(1)
        );
    }

    // ---------------------------------------------------------------
    // Выравнивание абзаца (^J) — фиксированная ширина, см. NANO.md:
    // в оригинале ширина = ширина терминала, здесь для простоты
    // константа, а не текущая ширина панели.
    // ---------------------------------------------------------------
    fn justify(&mut self) {
        const WRAP_WIDTH: usize = 72;

        if self.lines[self.cursor_line].trim().is_empty() {
            self.status = String::from("Nothing to justify (empty line)");
            return;
        }

        let mut start = self.cursor_line;
        while start > 0 && !self.lines[start - 1].trim().is_empty() {
            start -= 1;
        }
        let mut end = self.cursor_line;
        while end + 1 < self.lines.len() && !self.lines[end + 1].trim().is_empty() {
            end += 1;
        }

        let words: Vec<&str> = self.lines[start..=end]
            .iter()
            .flat_map(|l| l.split_whitespace())
            .collect();
        if words.is_empty() {
            return;
        }

        let mut new_lines: Vec<String> = Vec::new();
        let mut current = String::new();
        for word in words {
            if current.is_empty() {
                current.push_str(word);
            } else if current.chars().count() + 1 + word.chars().count() <= WRAP_WIDTH {
                current.push(' ');
                current.push_str(word);
            } else {
                new_lines.push(current);
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            new_lines.push(current);
        }

        let new_len = new_lines.len();
        self.lines.splice(start..=end, new_lines);
        self.cursor_line = start + new_len - 1;
        self.cursor_col = char_len(&self.lines[self.cursor_line]);
        self.want_col = self.cursor_col;
        self.modified = true;
        self.status = String::from("Justified paragraph");
    }

    // ---------------------------------------------------------------
    // Сохранение
    // ---------------------------------------------------------------
    fn write_to(&mut self, path: &str) {
        let mut content = self.lines.join("\n");
        content.push('\n');
        match fs::write(path, content.as_bytes()) {
            Ok(()) => {
                self.filename = Some(path.to_string());
                self.modified = false;
                self.status = format!("Wrote {} lines", self.lines.len());
            }
            Err(e) => {
                self.status = format!("Error writing '{path}': {e}");
            }
        }
    }

    /// Раскраска для draw_nano*: заголовок в трёх частях, как
    /// titlebar() в оригинале ("File:"/"DIR:"/"" + путь + состояние).
    pub fn title_parts(&self) -> (&'static str, String, &'static str) {
        let prefix = if self.filename.is_some() { "File:" } else { "" };
        let path = self.filename.clone().unwrap_or_else(|| String::from("New Buffer"));
        let state = if self.modified { "Modified" } else { "" };
        (prefix, path, state)
    }

    /// Текст для нижней "статус-строки": либо последнее сообщение
    /// (аналог statusbar() без активного запроса), либо текст текущего
    /// prompt'а вместе с уже введённым вводом (аналог того же
    /// statusbar(), но используемого как строка ввода — так делает и
    /// оригинал, у него это буквально одна и та же строка экрана).
    pub fn prompt_line(&self) -> String {
        match &self.prompt {
            None | Some(Prompt::Help) => self.status.clone(),
            Some(Prompt::SaveAs { input }) => format!("File Name to Write: {input}"),
            Some(Prompt::ExitConfirm) => {
                String::from("Save modified buffer?   Y Yes   N No   ^C Cancel")
            }
            Some(Prompt::Search { input }) => format!("Search: {input}"),
            Some(Prompt::ReplaceFind { input }) => format!("Search (to replace): {input}"),
            Some(Prompt::ReplaceWith { input, .. }) => format!("Replace with: {input}"),
            Some(Prompt::Goto { input }) => format!("Enter line number, column number: {input}"),
            Some(Prompt::InsertFile { input }) => format!("File to insert: {input}"),
        }
    }

    /// Есть ли активный ввод (курсор блоком в конце строки нужно
    /// рисовать только для реальных текстовых prompt'ов, не для
    /// статус-сообщений и не для Y/N/^C-подтверждения).
    pub fn prompt_has_cursor(&self) -> bool {
        matches!(
            self.prompt,
            Some(Prompt::SaveAs { .. })
                | Some(Prompt::Search { .. })
                | Some(Prompt::ReplaceFind { .. })
                | Some(Prompt::ReplaceWith { .. })
                | Some(Prompt::Goto { .. })
                | Some(Prompt::InsertFile { .. })
        )
    }
}
