use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    error::Error,
    io,
    time::{Duration, Instant}, iter::zip, sync::mpsc::{self, Receiver, Sender},
};
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    symbols,
    text::Span,
    widgets::{Axis, Block, Borders, Chart, Dataset},
    Frame, Terminal,
};
use cpal::traits::{HostTrait, DeviceTrait, StreamTrait};
use cpal::{Data, Sample, SampleFormat, FromSample};
use closure::closure;


const BUFFSIZE:usize = 1024;


struct App {
    waveform_snapshot: [(f32, f32); BUFFSIZE],
    window: [f64; 2],
}

impl App {
    fn new() -> App {
        App {
            waveform_snapshot: [(0.0, 0.0);BUFFSIZE],
            window: [0.0, 63555000.0],
        }
    }

    fn on_tick(&mut self) {
        self.window[0] = self.waveform_snapshot[0].0 as f64;
        self.window[1] = self.waveform_snapshot[BUFFSIZE-1].0 as f64;
    }
}

fn main() -> Result<(), Box<dyn Error>> {

    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    

    //setup audio stream
    let host = cpal::default_host();
    let device = host.default_input_device().expect("No input device available");
    let mut supported_configs_range = device.supported_input_configs().expect("error while querying configs");
    let supported_config = supported_configs_range.next().expect("no supported config?!")
    .with_max_sample_rate();
    let err_fn = |err| eprintln!("an error occurred on the output audio stream: {}", err);
    let sample_format = supported_config.sample_format();
    let config = supported_config.into();
    //println!("{:?}", config);

    let (tx, rx): (Sender<[(f32, f32);BUFFSIZE]>, Receiver<[(f32, f32);BUFFSIZE]>) = mpsc::channel();
    let audio_tx = tx.clone();
    let mut prev_time = Instant::now();
    let mut time = 0.0;
    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(&config, 
            closure!(move mut time, move mut prev_time,   move audio_tx, |input:&[f32], callbackdata| {
                
                let timediff = (Instant::now().duration_since(prev_time)).as_micros() as f32;
                
                let mut amps= [0.0;BUFFSIZE];
                amps.clone_from_slice(&input[0..BUFFSIZE]);
                
                let mut out = [(0.0, 0.0);BUFFSIZE];
                for i in 0..BUFFSIZE{
                    let t = time + ((i+1) as f32 *timediff) as f32;
                    out[i] = (t, amps[i]);
                }

                audio_tx.send(out).unwrap(); 
                
                time += timediff;
                prev_time = Instant::now();
            }), 
            err_fn, None),
        sample_format => panic!("Unsupported sample format '{sample_format}'")
    }.unwrap();
    stream.play().unwrap();


    // create app and run it
    let tick_rate = Duration::from_millis(1);
    
    let app = App::new();
    let res = run_app(&mut terminal, app, tick_rate, rx);

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }

    Ok(())
}

fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
    tick_rate: Duration, 
     snapshot_rx: Receiver<[(f32, f32);BUFFSIZE]>
) -> io::Result<()> {
    let mut last_tick = Instant::now();
    
    loop {
        app.waveform_snapshot = snapshot_rx.recv().unwrap();

        terminal.draw(|f| ui(f, &app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if let KeyCode::Char('q') = key.code {
                    return Ok(());
                }
            }
        }
        if last_tick.elapsed() >= tick_rate {
            app.on_tick();
            last_tick = Instant::now();
        }
    }
}

fn ui<B: Backend>(f: &mut Frame<B>, app: &App) {
    let size = f.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Ratio(1, 3)
            ]
            .as_ref(),
        )
        .split(size);
    let x_labels = vec![
        Span::styled(
            format!("{}", app.window[0]),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("{}", (app.window[0] + app.window[1]) / 2.0)),
        Span::styled(
            format!("{}", app.window[1]),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ];
    let data = app.waveform_snapshot.iter().map(|&e| (e.0 as f64, e.1 as f64)).collect::<Vec::<(f64,f64)>>();
    let datasets = vec![
        Dataset::default()
            .name("waveform_snapshot")
            .marker(symbols::Marker::Dot)
            .style(Style::default().fg(Color::Cyan))
            .data(data.as_slice())
    ];

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .title(Span::styled(
                    "waveform",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL),
        )
        .x_axis(
            Axis::default()
                .title("Timestep")
                .style(Style::default().fg(Color::Gray))
                .labels(x_labels)
                .bounds(app.window),
        )
        .y_axis(
            Axis::default()
                .title("Amplitude")
                .style(Style::default().fg(Color::Gray))
                .labels(vec![
                    Span::styled("-1", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw("0"),
                    Span::styled("1", Style::default().add_modifier(Modifier::BOLD)),
                ])
                .bounds([-1.0, 1.0]),
        );
    f.render_widget(chart, chunks[0]);
    
}
