mod agg;
mod app;
mod cast;
mod data;
mod demo;
mod pricing;
mod report;
mod scan;
mod timeutil;
mod ui;
mod worker;

use agg::Range;
use app::App;
use data::Dataset;
use ratatui::crossterm::event::{self, Event};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};
use worker::{Msg, Worker};

const HELP: &str = "\
cc-usage — daily Claude Code token usage in your terminal

Read-only: reads Claude Code's transcripts (*.jsonl), writes no files, makes no network connections.

USAGE:
    cc-usage [OPTIONS]

OPTIONS:
    -r, --range <1d|7d|30d|90d|all>   Initial time range (default: 7d)
        --print                       Print a plain-text report and exit (no TUI)
        --days <N>                    With --print: number of days, 1-3650 (default: 14)
        --projects-dir <PATH>         Transcript directory (repeatable; default: ~/.claude/projects)
        --refresh <SECS>              Rescan interval (default: 3)
        --theme <NAME>                Colour theme: mainframe (default), amber, catppuccin
        --color <auto|truecolor|256>  Colour depth (default: auto, from $COLORTERM)
        --demo                        Use synthetic data instead of reading any transcripts
    -h, --help                        Show this help
    -V, --version                     Show version

Inside the TUI press ? for key bindings.
Costs are API-equivalent estimates; built-in prices were last checked {PRICES}.
";

struct Args {
    range: Range,
    print: bool,
    days: i32,
    dirs: Vec<PathBuf>,
    refresh: f64,
    theme: ui::theme::ThemeId,
    truecolor: Option<bool>,
    demo: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        range: Range::Week,
        print: false,
        days: 14,
        dirs: vec![],
        refresh: 3.0,
        theme: ui::theme::ThemeId::default(),
        truecolor: None,
        demo: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{}", HELP.replace("{PRICES}", pricing::PRICES_VERIFIED));
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("cc-usage {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-r" | "--range" => {
                a.range = match val("--range")?.as_str() {
                    "1d" | "today" => Range::Today,
                    "7d" | "week" => Range::Week,
                    "30d" | "month" => Range::Month,
                    "90d" | "quarter" => Range::Quarter,
                    "all" => Range::All,
                    other => return Err(format!("unknown range '{other}' (use 1d, 7d, 30d, 90d, all)")),
                }
            }
            "--print" => a.print = true,
            "--days" => {
                a.days = val("--days")?
                    .parse::<i32>()
                    .map_err(|_| "--days needs a number")?
                    .clamp(1, 3650);
            }
            "--projects-dir" => a.dirs.push(PathBuf::from(val("--projects-dir")?)),
            "--refresh" => {
                let secs: f64 = val("--refresh")?.parse().map_err(|_| "--refresh needs a number")?;
                if !secs.is_finite() {
                    return Err("--refresh needs a number".into());
                }
                a.refresh = secs.clamp(0.5, 3600.0);
            }
            "--theme" => {
                let v = val("--theme")?;
                a.theme = ui::theme::ThemeId::parse(&v)
                    .ok_or_else(|| format!("unknown theme '{v}' (choose from: {})", ui::theme::ThemeId::list()))?;
            }
            "--color" => {
                a.truecolor = match val("--color")?.as_str() {
                    "auto" => None,
                    "truecolor" | "24bit" => Some(true),
                    "256" => Some(false),
                    other => return Err(format!("unknown --color '{other}' (use auto, truecolor, 256)")),
                }
            }
            "--demo" => a.demo = true,
            other => return Err(format!("unknown argument '{other}' (try --help)")),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("cc-usage: {e}");
            return ExitCode::from(2);
        }
    };
    let roots = if args.dirs.is_empty() {
        scan::default_roots()
    } else {
        args.dirs.clone()
    };
    if roots.is_empty() && !args.demo {
        eprintln!("cc-usage: no transcript directory found (looked for ~/.claude/projects); use --projects-dir");
        return ExitCode::FAILURE;
    }

    if args.print {
        let ds = if args.demo {
            demo::dataset(chrono::Utc::now().timestamp())
        } else {
            let mut sc = scan::Scanner::new(roots);
            sc.scan();
            Dataset::build(&sc.files)
        };
        print!("{}", report::render(&ds, args.days.max(1), timeutil::today()));
        return ExitCode::SUCCESS;
    }

    let mut app = App::new(args.range);
    app.theme = args.theme;
    app.truecolor = args.truecolor.unwrap_or_else(ui::theme::detect_truecolor);
    let worker = if args.demo {
        app.roots = vec!["(demo data)".into()];
        app.on_msg(Msg::Data(Arc::new(demo::dataset(chrono::Utc::now().timestamp()))));
        app.on_msg(Msg::Scanning(false));
        None
    } else {
        app.roots = roots.iter().map(|p| p.display().to_string()).collect();
        Some(Worker::spawn(roots, Duration::from_secs_f64(args.refresh)))
    };

    let mut terminal = ratatui::init();
    let res = run(&mut terminal, &mut app, worker.as_ref());
    ratatui::restore();
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cc-usage: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App, worker: Option<&Worker>) -> std::io::Result<()> {
    let mut last_tick = Instant::now();
    loop {
        if app.dirty {
            terminal.draw(|f| ui::draw(f, app))?;
            app.dirty = false;
        }
        if event::poll(Duration::from_millis(100))? {
            // Drain bursts (held keys, resize storms) before redrawing once.
            loop {
                match event::read()? {
                    Event::Key(k) => app.on_key(k),
                    Event::Resize(..) => app.dirty = true,
                    _ => {}
                }
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        if let Some(worker) = worker {
            while let Ok(m) = worker.rx.try_recv() {
                app.on_msg(m);
            }
            if app.rescan_requested {
                app.rescan_requested = false;
                worker.rescan();
            }
        }
        if last_tick.elapsed() >= Duration::from_secs(1) {
            last_tick = Instant::now();
            app.tick();
        }
        if app.quit {
            return Ok(());
        }
    }
}
