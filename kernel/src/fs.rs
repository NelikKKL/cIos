//! Простая файловая система в ОЗУ (RAM-FS). Настоящего диска пока нет
//! (Phase 6 будет про персистентное хранилище) — при перезагрузке всё
//! содержимое теряется. Этого достаточно, чтобы ls/cat/rm/mkdir/touch/cd
//! работали по-настоящему, а не были заглушками.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;

pub enum Node {
    File(Vec<u8>),
    /// Дочерние элементы как (имя, узел) — без индекса по хэшу,
    /// каталогов немного, линейный поиск вполне достаточен.
    Dir(Vec<(String, Node)>),
}

static ROOT: Mutex<Option<Node>> = Mutex::new(None);

/// Создаёт корень с парой демо-файлов. Вызывать один раз при старте
/// ядра, после memory::init() (нужна куча).
pub fn init() {
    let root = Node::Dir(alloc::vec![
        (
            String::from("readme.txt"),
            Node::File(b"welcome to CIOS.\ntype 'help' for a list of commands.\n".to_vec()),
        ),
        (String::from("home"), Node::Dir(Vec::new())),
    ]);
    *ROOT.lock() = Some(root);
}

/// Превращает относительный/абсолютный путь в нормализованный
/// абсолютный путь (понимает "..", ".", повторные "/").
pub fn resolve(cwd: &str, target: &str) -> String {
    let mut stack: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        cwd.split('/').filter(|s| !s.is_empty()).collect()
    };
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            p => stack.push(p),
        }
    }
    if stack.is_empty() {
        String::from("/")
    } else {
        format!("/{}", stack.join("/"))
    }
}

fn split_path(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

fn find<'a>(root: &'a Node, parts: &[&str]) -> Option<&'a Node> {
    let mut current = root;
    for part in parts {
        match current {
            Node::Dir(children) => {
                current = &children.iter().find(|(name, _)| name.as_str() == *part)?.1;
            }
            Node::File(_) => return None,
        }
    }
    Some(current)
}

/// Находит родительскую директорию (мутабельно) и имя последнего
/// компонента пути — нужно для rm/mkdir/touch.
fn find_parent_mut<'a>(root: &'a mut Node, parts: &[&str]) -> Option<(&'a mut Vec<(String, Node)>, String)> {
    let (last, dirs) = parts.split_last()?;
    let mut current = root;
    for part in dirs {
        match current {
            Node::Dir(children) => {
                current = &mut children.iter_mut().find(|(name, _)| name.as_str() == *part)?.1;
            }
            Node::File(_) => return None,
        }
    }
    match current {
        Node::Dir(children) => Some((children, last.to_string())),
        Node::File(_) => None,
    }
}

pub fn list(path: &str) -> Result<Vec<String>, String> {
    let guard = ROOT.lock();
    let root = guard.as_ref().expect("fs not initialized");
    let parts = split_path(path);
    match find(root, &parts) {
        Some(Node::Dir(children)) => {
            let mut names: Vec<String> = children
                .iter()
                .map(|(name, node)| match node {
                    Node::Dir(_) => format!("{}/", name),
                    Node::File(_) => name.clone(),
                })
                .collect();
            names.sort();
            Ok(names)
        }
        Some(Node::File(_)) => Err(String::from("not a directory")),
        None => Err(String::from("no such file or directory")),
    }
}

pub fn read(path: &str) -> Result<Vec<u8>, String> {
    let guard = ROOT.lock();
    let root = guard.as_ref().expect("fs not initialized");
    let parts = split_path(path);
    match find(root, &parts) {
        Some(Node::File(content)) => Ok(content.clone()),
        Some(Node::Dir(_)) => Err(String::from("is a directory")),
        None => Err(String::from("no such file or directory")),
    }
}

pub fn is_dir(path: &str) -> bool {
    let guard = ROOT.lock();
    let root = match guard.as_ref() {
        Some(r) => r,
        None => return false,
    };
    let parts = split_path(path);
    matches!(find(root, &parts), Some(Node::Dir(_)))
}

pub fn remove(path: &str) -> Result<(), String> {
    let mut guard = ROOT.lock();
    let root = guard.as_mut().expect("fs not initialized");
    let parts = split_path(path);
    if parts.is_empty() {
        return Err(String::from("cannot remove root"));
    }
    let (children, name) =
        find_parent_mut(root, &parts).ok_or_else(|| String::from("no such file or directory"))?;
    let idx = children
        .iter()
        .position(|(n, _)| n == &name)
        .ok_or_else(|| String::from("no such file or directory"))?;
    children.remove(idx);
    Ok(())
}

pub fn make_dir(path: &str) -> Result<(), String> {
    let mut guard = ROOT.lock();
    let root = guard.as_mut().expect("fs not initialized");
    let parts = split_path(path);
    if parts.is_empty() {
        return Err(String::from("cannot create root"));
    }
    let (children, name) =
        find_parent_mut(root, &parts).ok_or_else(|| String::from("no such parent directory"))?;
    if children.iter().any(|(n, _)| n == &name) {
        return Err(String::from("already exists"));
    }
    children.push((name, Node::Dir(Vec::new())));
    Ok(())
}

pub fn make_file(path: &str) -> Result<(), String> {
    let mut guard = ROOT.lock();
    let root = guard.as_mut().expect("fs not initialized");
    let parts = split_path(path);
    if parts.is_empty() {
        return Err(String::from("cannot create root"));
    }
    let (children, name) =
        find_parent_mut(root, &parts).ok_or_else(|| String::from("no such parent directory"))?;
    if children.iter().any(|(n, _)| n == &name) {
        return Err(String::from("already exists"));
    }
    children.push((name, Node::File(Vec::new())));
    Ok(())
}
