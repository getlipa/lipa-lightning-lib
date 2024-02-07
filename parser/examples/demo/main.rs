use crossterm::cursor::MoveToColumn;
use crossterm::event::{read, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::style::{Print, Stylize};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType};
use parser::{parse_lightning_address, ParseError};
use std::io;
use std::io::Write;

fn parse_and_print(line: &str) -> io::Result<()> {
    let mut stdout = io::stdout();
    match parse_lightning_address(&line) {
        Ok(()) => execute!(stdout, Print(line.green())),
        Err(ParseError::Incomplete) => execute!(stdout, Print(line.white())),
        Err(ParseError::ExcessSuffix(at)) => execute!(
            stdout,
            Print(line[..at].green()),
            Print(line[at..].black().on_magenta())
        ),
        Err(ParseError::UnexpectedCharacter(at)) => execute!(
            stdout,
            Print(line[..at].white()),
            Print(line[at..].black().on_red())
        ),
    }
}

fn print_events() -> io::Result<()> {
    let mut stdout = io::stdout();
    print!("{}", "Start typing a lightning address".italic().dim());
    execute!(stdout, MoveToColumn(0))?;

    let mut line = String::new();
    loop {
        let event = read()?;
        if let Event::Key(key_event) = event {
            match key_event.code {
                KeyCode::Char(c) if key_event.modifiers == KeyModifiers::NONE => {
                    execute!(stdout, Clear(ClearType::CurrentLine), MoveToColumn(0))?;
                    line.push(c);
                    parse_and_print(&line)?;
                }
                KeyCode::Backspace => {
                    execute!(stdout, Clear(ClearType::CurrentLine), MoveToColumn(0))?;
                    line.pop();
                    parse_and_print(&line)?;
                }
                KeyCode::Enter => {
                    line.clear();
                    println!();
                    execute!(stdout, MoveToColumn(0))?;
                }
                KeyCode::Char('d' | 'c') if key_event.modifiers == KeyModifiers::CONTROL => break,
                KeyCode::Esc => break,
                _ => (),
            }
        }
        stdout.flush()?;
    }
    println!();
    execute!(stdout, MoveToColumn(0))
}

fn main() -> io::Result<()> {
    println!("{}", "Press ESC to exit".bold());
    println!();
    println!("       Valid complete is {}", "green".green());
    println!("         Valid prefix is {}", "white".white());
    println!(
        "     Excess suffix is on {}",
        "magneta".black().on_magenta()
    );
    println!("Invalid charactes are on {}", "red".black().on_red());
    println!();

    enable_raw_mode()?;
    if let Err(e) = print_events() {
        eprintln!("Error: {e:?}");
    }
    disable_raw_mode()
}
