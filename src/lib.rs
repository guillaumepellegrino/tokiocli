//! A Unix [`Cli`] based on Tokio.
//!
//! This crate provide a simple library allowing to write
//! interactive Command Line Interface in an Unix spirit.
//!
//! APIs are async and thus can be easily integrated in a Tokio crate.
use eyre::Result;
use termios::*;
use tokio::io::{stdin, AsyncReadExt, BufReader, Stdin};

/** An Action performed by the user: execute a command or auto-complete the current command. */
pub enum Action {
    /** User demand to execute the following command (Command Name + Arguments). */
    Command(Vec<String>),
    /** User demand to auto-complete the following command (Command Name + Arguments). */
    AutoComplete(Vec<String>),
}

/** Human-readable ANSI Escape Sequences */
#[allow(dead_code)]
enum EscSeq {
    Up(usize),
    Down(usize),
    Right(usize),
    Left(usize),
    HorizontalAbs(usize),
    EraseInDisplay(usize),
    EraseInLineFromCursorToEnd,
    EraseInLineFromCursorToBegining,
    EraseInLineAll,
}

impl std::fmt::Display for EscSeq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Up(value) => write!(f, "\x1B[{}A", value),
            Self::Down(value) => write!(f, "\x1B[{}B", value),
            Self::Right(value) => write!(f, "\x1B[{}C", value),
            Self::Left(value) => write!(f, "\x1B[{}D", value),
            Self::HorizontalAbs(value) => write!(f, "\x1B[{}G", value),
            Self::EraseInDisplay(value) => write!(f, "\x1B[{}J", value),
            Self::EraseInLineFromCursorToEnd => write!(f, "\x1B[0K"),
            Self::EraseInLineFromCursorToBegining => write!(f, "\x1B[1K"),
            Self::EraseInLineAll => write!(f, "\x1B[2K"),
        }
    }
}

/** Provide APIs to interact with the Command Line Interface */
pub struct Cli {
    saved_termios: Termios,
    reader: BufReader<Stdin>,
    do_reset: bool,
    prompt: String,
    cmd: String,
    cursor: usize,
    history: Vec<String>,
    history_idx: Option<usize>,
}

impl Cli {
    /**
     * Create a new Command Line Interface.
     *
     * Note that it configures the terminal in character mode.
     */
    pub fn new() -> Result<Self> {
        let fd = 0;
        let saved = Termios::from_fd(fd)?;
        let mut termios = saved;
        termios.c_lflag &= !(ECHO | ECHONL | ICANON);
        tcsetattr(fd, TCSANOW, &termios)?;

        Ok(Self {
            saved_termios: saved,
            reader: BufReader::new(stdin()),
            do_reset: true,
            prompt: String::from("> "),
            cmd: String::new(),
            cursor: 0,
            history: Vec::<String>::new(),
            history_idx: None,
        })
    }

    fn cmd2args(&self) -> Vec<String> {
        let mut args = Vec::<String>::new();
        let mut arg = String::new();
        let mut is_string = false;
        let mut is_escaped = false;
        for c in self.cmd.chars() {
            if is_escaped {
                arg.push(c);
                is_escaped = false;
                continue;
            }
            match c {
                '\\' => {
                    is_escaped = true;
                }
                '"' => {
                    is_string = !is_string;
                }
                ' ' => {
                    match is_string {
                        true => {
                            arg.push(c);
                        }
                        false => {
                            args.push(arg.clone());
                            arg.clear();
                        }
                    };
                }
                _ => {
                    arg.push(c);
                }
            }
        }
        args.push(arg);
        args
    }

    fn clear_line(&self) -> Result<()> {
        eprint!("{}{}", EscSeq::EraseInLineAll, EscSeq::HorizontalAbs(0));
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        self.cmd.clear();
        self.cursor = 0;
        self.history_idx = None;
        eprint!("{}", self.prompt);
        Ok(())
    }

    fn history_restore(&mut self) -> Result<()> {
        let word = match self.history_idx {
            Some(idx) => &self.history[idx],
            None => {
                return Ok(());
            }
        };

        self.cmd = word.clone();
        self.cursor = match self.cmd.len() {
            0 => 0,
            len => len,
        };
        self.clear_line()?;
        eprint!("{}{}", self.prompt, self.cmd);

        Ok(())
    }

    async fn history_prev(&mut self) -> Result<()> {
        self.history_idx = match self.history_idx {
            Some(idx) => match idx {
                0 => Some(idx),
                idx => Some(idx - 1),
            },
            None => match self.history.len() {
                0 => None,
                idx => Some(idx - 1),
            },
        };

        self.history_restore()
    }

    async fn history_next(&mut self) -> Result<()> {
        self.history_idx = match self.history_idx {
            Some(idx) => {
                if (idx + 1) < self.history.len() {
                    Some(idx + 1)
                } else {
                    None
                }
            }
            None => None,
        };

        self.history_restore()
    }

    async fn cursor_reset(&mut self) -> Result<()> {
        eprint!("{}", EscSeq::Left(self.cursor));
        self.cursor = 0;
        Ok(())
    }

    async fn cursor_left(&mut self) -> Result<()> {
        if self.cursor > 0 {
            eprint!("{}", EscSeq::Left(1));
            self.cursor -= 1;
        }
        Ok(())
    }

    async fn cursor_right(&mut self) -> Result<()> {
        if self.cursor < self.cmd.len() {
            eprint!("{}", EscSeq::Right(1));
            self.cursor += 1;
        }
        Ok(())
    }

    async fn escape(&mut self) -> Result<()> {
        let c = self.reader.read_u8().await?;
        if c != 0x5B {
            return Ok(());
        }
        let c = self.reader.read_u8().await?;
        match c {
            0x33 => {
                // SUPPR
                self.suppr().await?;
            }
            0x41 => {
                // UP
                self.history_prev().await?;
            }
            0x42 => {
                // LOW
                self.history_next().await?;
            }
            0x43 => {
                // RIGHT
                self.cursor_right().await?;
            }
            0x44 => {
                // LEFT
                self.cursor_left().await?;
            }
            _ => {
                eprintln!("Unhandled ANSI Escape Sequence: {}", c);
            }
        }
        Ok(())
    }

    async fn addchar(&mut self, c: char) -> Result<()> {
        if self.cursor < self.cmd.len() {
            let right = &self.cmd[self.cursor..];
            eprint!("{}{}{}", c, right, EscSeq::Left(right.len()));
        } else {
            eprint!("{}", c);
        }

        self.cmd.insert(self.cursor, c);
        self.cursor += 1;
        Ok(())
    }

    async fn backspace(&mut self) -> Result<()> {
        if self.cursor == 0 {
            return Ok(());
        }

        let right = &self.cmd[self.cursor..];
        self.cursor -= 1;
        eprint!("\x08{} {}", right, EscSeq::Left(right.len() + 1));
        self.cmd.remove(self.cursor);

        Ok(())
    }

    async fn suppr(&mut self) -> Result<()> {
        let c = self.reader.read_u8().await? as char;
        if c != '~' {
            eprintln!("Unexpect character {}", c);
            return Ok(());
        }
        if self.cursor + 1 < self.cmd.len() {
            let right = &self.cmd[self.cursor + 1..];
            eprint!("{} {}", right, EscSeq::Left(right.len() + 1));
            self.cmd.remove(self.cursor);
        }
        Ok(())
    }

    async fn eol(&mut self) -> Result<Vec<String>> {
        eprintln!();
        let args = self.cmd2args();
        if !args[0].is_empty() {
            self.history.push(self.cmd.clone());
        }
        Ok(args)
    }

    /**
     * Return an Action demanded by the user in CLI.
     */
    pub async fn getaction(&mut self) -> Result<Action> {
        if self.do_reset {
            self.reset()?;
            self.do_reset = false;
        }
        loop {
            let c = self.reader.read_u8().await?;

            match c {
                0x01 | 0x02 => {
                    self.cursor_reset().await?;
                }
                0x1B => {
                    // ESC (escap)
                    self.escape().await?;
                }
                0x7F => {
                    // DEL
                    self.backspace().await?;
                }
                b'\n' => {
                    self.do_reset = true;
                    return Ok(Action::Command(self.eol().await?));
                }
                b'\t' => {
                    return Ok(Action::AutoComplete(self.cmd2args()));
                }
                _ => {
                    self.addchar(c as char).await?;
                }
            }
        }
    }

    /*
    /** Get the list of arguments inputed by User. */
    pub async fn getargs(self: &mut Self) -> Result<Vec<String>> {
        loop {
            let action = self.getaction().await?;
            match action {
                Action::Command(args) => {return Ok(args);},
                Action::AutoComplete(args) => {
                    self.autocomplete(&Vec::<String>::new());
                },
            }
        }
    }
    */

    /** Auto-complete the current command with the provided list of possible words */
    pub fn autocomplete(&mut self, words: &Vec<String>) -> Result<()> {
        if words.is_empty() {
            // Nothing to do
            return Ok(());
        }

        // Retrieve common word
        let mut common = words[0].as_str();
        for word in words {
            common = common_chars(word, common);
        }

        // Get completion word from common word
        let args = self.cmd2args();
        let lastarg = args.last().unwrap();
        let complete = &common[lastarg.len()..];

        if words.len() == 1 {
            // Complete current line
            self.cmd += complete;
            self.cursor += complete.len();
            eprint!("{}", complete);
        } else {
            // Display all possibilites
            eprintln!();
            for word in words {
                eprint!("{} ", word);
            }
            // Write back partially completed command
            self.cmd += complete;
            self.cursor += complete.len();
            eprint!("\n{}{}", self.prompt, self.cmd);
        }

        Ok(())
    }

    /** Set the name of the prompt */
    pub fn setprompt(&mut self, prompt: &str) -> &mut Self {
        self.prompt = prompt.into();
        self
    }
}

impl Drop for Cli {
    /**
     * Release Cli ressources and configure back the terminal in its orignal state.
     */
    fn drop(&mut self) {
        let fd = 0;
        if let Err(e) = tcsetattr(fd, TCSANOW, &self.saved_termios) {
            eprintln!("Failed to restore terminal config: {:?}", e);
        }
    }
}

fn common_chars<'a>(lstr: &'a str, rstr: &'_ str) -> &'a str {
    let lindices = lstr.char_indices();
    let mut rindices = rstr.char_indices();
    let mut common = 0;

    for (_, lchar) in lindices {
        match rindices.next() {
            Some((_, rchar)) => {
                if lchar != rchar {
                    break;
                }
                common += 1;
            }
            None => {
                break;
            }
        };
    }

    &lstr[0..common]
}
