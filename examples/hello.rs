use tokiocli::{Action, Cli};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let mut cli = Cli::new()?;
    let mut exit = false;

    while !exit {
        let action = cli.getaction().await?;
        match action {
            Action::Command(cmd) => runcmd(cmd, &mut exit),
            Action::AutoComplete(cmd) => autocomplete(&mut cli, cmd)?,
            Action::NoAction => exit = true,
        };
    }

    Ok(())
}

fn runcmd(cmd: Vec<String>, exit: &mut bool) {
    let cmdname = cmd.get(0).unwrap();
    match cmdname.as_str() {
        "hello" => {
            println!("Hello from tokiocli");
        }
        "upper" => upper(cmd),
        "exit" => {
            *exit = true;
        }
        "help" => help(),
        "" => {}
        _ => {
            println!(
                "Unknown command '{}'.\nType 'help' to list available commands.",
                cmdname
            );
        }
    }
}

fn autocomplete(cli: &mut Cli, cmd: Vec<String>) -> eyre::Result<()> {
    let cmdlist = vec![
        String::from("hello"),
        String::from("upper"),
        String::from("exit"),
        String::from("help"),
    ];

    if cmd.len() == 1 {
        // autocomplete command
        cli.autocomplete(&cmdlist)?;
    } else if cmd.len() > 1 {
        // autocomplete arguments
    }
    Ok(())
}

fn upper(cmd: Vec<String>) {
    for arg in cmd.iter().skip(1) {
        print!("{} ", arg.to_uppercase());
    }
    println!("");
}

fn help() {
    println!("Simple interactive cli example.");
    println!("Available commands:");
    println!("  hello: Print hello world");
    println!("  upper: Print arguments to upper case");
    println!("  exit: Exit this application");
    println!("  help: Display this help");
}
