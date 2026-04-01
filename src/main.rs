pub mod terminal;

use std::io::{Read, Write};
use crate::terminal::{Bash, Terminal};
fn main() {
    let mut term = Bash::new("/data/data/com.termux/files/home".to_string());
    term.run();
}