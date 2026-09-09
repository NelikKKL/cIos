//! Разбор и выполнение командных строк шелла. Команды работают поверх
//! in-memory файловой системы (fs.rs). Без пайпов/редиректов/кавычек
//! пока — это отдельная задача, если понадобится.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::fs;

/// Список команд для `help` и, позже, автодополнения.
pub const COMMANDS: &[&str] = &["ls", "cat", "rm", "mkdir", "touch", "cd", "pwd", "echo", "clear", "help", "file-sys"];

/// Выполняет одну командную строку, дописывая построчный вывод в
/// `history`. `cwd` — текущая директория (абсолютный путь, всегда
/// начинается с '/'), может быть изменена командой `cd`.
pub fn execute(line: &str, cwd: &mut String, history: &mut Vec<String>) {
    let mut parts = line.split_whitespace();
    let cmd = match parts.next() {
        Some(c) => c,
        None => return,
    };
    let args: Vec<&str> = parts.collect();

    match cmd {
        "help" => {
            history.push(format!("commands: {}", COMMANDS.join(" ")));
        }
        "pwd" => {
            history.push(cwd.clone());
        }
        "ls" => {
            let target = args.first().copied().unwrap_or(".");
            let path = fs::resolve(cwd, target);
            match fs::list(&path) {
                Ok(names) if names.is_empty() => history.push(String::from("(empty)")),
                Ok(names) => history.push(names.join("  ")),
                Err(e) => history.push(format!("ls: {target}: {e}")),
            }
        }
        "cat" => match args.first() {
            Some(target) => {
                let path = fs::resolve(cwd, target);
                match fs::read(&path) {
                    Ok(content) => {
                        let text = String::from_utf8_lossy(&content);
                        for l in text.lines() {
                            history.push(String::from(l));
                        }
                    }
                    Err(e) => history.push(format!("cat: {target}: {e}")),
                }
            }
            None => history.push(String::from("usage: cat <path>")),
        },
        "rm" => match args.first() {
            Some(target) => {
                let path = fs::resolve(cwd, target);
                if let Err(e) = fs::remove(&path) {
                    history.push(format!("rm: {target}: {e}"));
                }
            }
            None => history.push(String::from("usage: rm <path>")),
        },
        "mkdir" => match args.first() {
            Some(target) => {
                let path = fs::resolve(cwd, target);
                if let Err(e) = fs::make_dir(&path) {
                    history.push(format!("mkdir: {target}: {e}"));
                }
            }
            None => history.push(String::from("usage: mkdir <path>")),
        },
        "touch" => match args.first() {
            Some(target) => {
                let path = fs::resolve(cwd, target);
                if let Err(e) = fs::make_file(&path) {
                    history.push(format!("touch: {target}: {e}"));
                }
            }
            None => history.push(String::from("usage: touch <path>")),
        },
        "cd" => {
            let target = args.first().copied().unwrap_or("/");
            let path = fs::resolve(cwd, target);
            if fs::is_dir(&path) {
                *cwd = path;
            } else {
                history.push(format!("cd: {target}: not a directory"));
            }
        }
        "echo" => {
            history.push(args.join(" "));
        }
        "clear" => {
            history.clear();
        }
        "file-sys" => {
            // Основной вход — через main.rs (голое 'file-sys' + Enter
            // переключает главный цикл в интерактивный режим со своим
            // рендером и обработкой стрелок). Сюда попадаем только если
            // после 'file-sys' были лишние аргументы.
            history.push(String::from("file-sys takes no arguments — just run 'file-sys'"));
        }
        other => {
            history.push(format!("{other}: command not found"));
        }
    }
}
