use std::sync::{Arc, Mutex};
use std::any::Any;
use std::thread::{self, spawn};
use std::process::{ChildStderr, ChildStdout, Command, Stdio, exit};
use std::io::{self, BufRead, BufReader, Read, Write, stderr, stdout};
use std::{array, collections::HashMap, hash::Hash, ops::DerefMut, thread::sleep, time::{Duration, Instant}};
use std::path::{Path, PathBuf};
use std::env;
use crossterm::cursor::{MoveDown, MoveLeft, MoveToColumn};
use crossterm::style::{StyledContent, Stylize};
use crossterm::{
    execute, queue,
    terminal::{size, SetSize, Clear, ClearType, enable_raw_mode, disable_raw_mode, is_raw_mode_enabled},
    style::{Print, SetBackgroundColor, SetForegroundColor, ResetColor, Color as TermColor},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    cursor::{MoveTo, Hide, position}
};

pub enum TerminalEvent {
    Char { c: char },
    Backspace,
    Enter,
    Right,
    Left,
    Up,
    Down
}

pub trait Terminal {
    fn build_prefix(&mut self) -> String;
    fn update_path(&mut self);
    fn emit(&mut self, ev: TerminalEvent);
    fn get_event(&mut self);
    fn write(&self, content: &str);
    fn flush(&self);
    fn writenflush(&self, content: &str);
    fn add(&mut self, content: &str) {
        for c in content.chars() {self.emit(TerminalEvent::Char { c });}
    }
    fn execute(&mut self, command: &str);
    fn enter(&mut self) {self.emit(TerminalEvent::Enter);}
    fn backspace(&mut self) {self.emit(TerminalEvent::Backspace);}
    fn run(&mut self);
}

pub struct Bash {
    pub path: String,
    pub input_time: f32,
    pub cmdbuff: String
}

impl Bash {
    pub fn new(path: String) -> Bash {
        Bash {
            path: path,
            input_time: 0.0001,
            cmdbuff: String::new()
        }
    }
    fn format_prefix(&mut self) -> String {
        format!("{} $ ", self.build_prefix().green().to_string())
    }
    fn len_prefix(&mut self) -> usize {
        format!("{} $ ", self.build_prefix()).len()
    }
}

impl Terminal for Bash {
    fn writenflush(&self, content: &str) {self.write(content);self.flush();}
    fn write(&self, content: &str) {io::stdout().write_all(content.as_bytes());}
    fn flush(&self) {io::stdout().flush();}
    fn update_path(&mut self) {
        self.path = String::from(env::current_dir().unwrap_or_else(|x| PathBuf::new()).to_str().unwrap_or_else(|| ""));
    }
    fn execute(&mut self, command: &str) {
        let mut proc = Command::new(if cfg!(windows) { "cmd.exe" } else { "bash" })
            .args(if cfg!(windows) {
                vec!["/C", command]
            } else {
                vec!["-c", command]
            })
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = proc.stdout.take().expect("oops!");
        let stderr = proc.stderr.take().expect("oops!");
        let outreader = BufReader::new(stdout);
        let errreader = BufReader::new(stderr);
        
        let mut hasout_err = Arc::new(Mutex::new(false));
        let mut hasout_out = hasout_err.clone();
        thread::spawn(move || {
            for line in errreader.lines() {
                match line {
                    Ok(l) => {
                        if !*hasout_err.lock().unwrap() {
                            io::stdout().write(b"\n");
                            io::stdout().write(l.as_bytes());
                            io::stdout().flush();
                            *hasout_err.lock().unwrap() = true;
                        } else {
                            io::stdout().write(l.as_bytes());
                            io::stdout().flush();
                        }
                    }
                    Err(l) => {}
                }
            }
        });

        for line in outreader.lines() {
            match line {
                Ok(l) => {
                    if !*hasout_out.lock().unwrap() {
                        self.write("\n");
                        self.writenflush(l.as_str());
                        *hasout_out.lock().unwrap() = true;
                    } else {
                        self.writenflush(l.as_str());
                    }
                }
                Err(l) => {}
            }
        }
        let status = proc.wait().expect("failed to wait on child");
    }
    fn build_prefix(&mut self) -> String {
        let path = Path::new(self.path.as_str());
        let home = env::var("HOME")
            .or_else(|_| env::var("USERPROFILE"))
            .unwrap_or_else(|_| "/".to_string());
        let home = PathBuf::from(home);

        let abs_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        let display_path = if abs_path.starts_with(&home) {
            PathBuf::from("~").join(abs_path.strip_prefix(&home).unwrap())
        } else {
            abs_path
        };

        let parts: Vec<_> = display_path
            .components()
            .filter_map(|c| {
                use std::path::Component::*;
                match c {
                    RootDir => None,
                    Normal(s) if !s.is_empty() => Some(s.to_string_lossy().into_owned()),
                    CurDir => None,
                    ParentDir => Some("..".to_string()),
                    Prefix(_) => None,
                    _ => None
                }
            })
            .collect();
        
        let mut res = String::new();
        if parts.len() > 3 {
            if parts[0] == "~" {
                res = format!("{} /.../{}", parts[0], parts[parts.len()-2..].join("/"));
            } else {
                res = format!(".../{}", parts[parts.len()-2..].join("/"));
            }
        } else {
            res = parts.join("/");
        }
        res
    } 
    fn emit(&mut self, ev: TerminalEvent) {
        match ev {
            TerminalEvent::Char { c } => {
                self.writenflush(&c.to_string());
                self.cmdbuff.push(c);
            },
            TerminalEvent::Enter => {
                self.write("\n");
                queue!(io::stdout(), MoveToColumn(0));
                let cmd = self.cmdbuff.clone();
                self.cmdbuff.clear();
                if !cmd.trim().is_empty() {
                    self.execute(&cmd.as_str());
                }
                let prefix = self.format_prefix();
                self.writenflush(prefix.as_str());
            }
            TerminalEvent::Backspace => {
                let prefix = self.build_prefix();
                let prefixlen = self.len_prefix();
                let curpos = position().unwrap();
                if self.len_prefix() == curpos.0 as usize {return;}
                queue!(
                    io::stdout(),
                    MoveLeft(1),
                    Print(' '),
                    MoveLeft(1)
                ).unwrap();
                self.flush();
            }
            _ => {}
        }
    }
    fn get_event(&mut self) {
        if event::poll(Duration::from_secs_f32(self.input_time)).unwrap() {
            let ev = event::read().unwrap();
            match ev {
                Event::Key(KeyEvent {code, kind, modifiers, ..}) => match kind {
                    KeyEventKind::Press => {
                        match code {
                            KeyCode::Char(c) if c == 'c' || c == 'z' => {
                                match modifiers {
                                    KeyModifiers::CONTROL => {
                                        match c {
                                            'c' => {
                                                self.emit(TerminalEvent::Char { c: '^' });
                                                self.emit(TerminalEvent::Char { c: c.to_uppercase().next().unwrap_or_else(|| c) });
                                                self.emit(TerminalEvent::Enter);
                                            }
                                            _ => {
                                                disable_raw_mode();
                                                exit(0);
                                            }
                                        }
                                    }
                                    KeyModifiers::NONE | _ => {
                                        self.emit(TerminalEvent::Char { c: c });
                                    }
                                }
                            }
                            KeyCode::Char(x) => {self.emit(TerminalEvent::Char { c: x });}
                            KeyCode::Enter => {self.emit(TerminalEvent::Enter);}
                            KeyCode::Backspace => {self.emit(TerminalEvent::Backspace);}
                            KeyCode::Up => {self.emit(TerminalEvent::Up);}
                            KeyCode::Down => {self.emit(TerminalEvent::Down);}
                            KeyCode::Left => {self.emit(TerminalEvent::Left);}
                            KeyCode::Right => {self.emit(TerminalEvent::Right);}
                            _ => {} 
                        }
                    },
                    _ => {}
                }
                _ => {}
            }
        }
    }
    fn run(&mut self) {
        self.update_path();

        enable_raw_mode().unwrap();
        let prefix = "\n".to_string() + self.format_prefix().as_str();
        self.writenflush(&prefix.as_str());
        loop {
            self.get_event();
            self.update_path();
        }
    }
}