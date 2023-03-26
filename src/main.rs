use std::{
    error::Error,
    io,
    thread,
    time::{Duration, Instant}
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
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
use clap::Parser;
use cpal::{traits::{HostTrait, DeviceTrait, StreamTrait}, SampleFormat, Device, SupportedStreamConfigRange};
use closure::closure;
use ringbuffer::{AllocRingBuffer, RingBufferWrite, RingBufferExt};
use bus::{Bus, BusReader};

mod pitchdetect;
mod midihandler;
//FIXME: allow for oversized buffer
const SNAPSHOT_BUFFLEN:usize = 1024; //882

const CONTOUR_BUFFLEN:usize = 128;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct AppArgs{

    #[arg(short, long, default_value_t = {"default".to_string()})]
    device_name: String,

    #[arg(short, long, default_value_t = 48000)]
    srate: usize,

    #[arg(short, long, default_value_t = 2.0)]
    power_thresh: f32,

    #[arg(short, long, default_value_t = 0.2)]
    clairty_thresh: f32,

    #[arg(short, long, default_value_t = false)]
    no_ui: bool
}

struct App {
    waveform_snapshot: [(f32, f32); SNAPSHOT_BUFFLEN],
    f0_contour: AllocRingBuffer<(f32, f32)>,
    wavviz_window: [f64; 2],
    f0_window: [f64; 2]
}

impl App {
    fn new() -> App {
        App {
            waveform_snapshot: [(0.0, 0.0);SNAPSHOT_BUFFLEN],
            wavviz_window: [0.0, 63555000.0],
            f0_contour: AllocRingBuffer::with_capacity(CONTOUR_BUFFLEN),
            f0_window: [0.0, 63555000.0]
        }
    }

    //update window bounds
    fn on_tick(&mut self) {
        self.wavviz_window[0] = self.waveform_snapshot[0].0 as f64;
        self.wavviz_window[1] = self.waveform_snapshot[SNAPSHOT_BUFFLEN-1].0 as f64;
        self.f0_window[0] = self.f0_contour.get(0).unwrap_or(&(0.0, 0.0)).0 as f64;
        self.f0_window[1] = self.f0_contour.get(-1).unwrap_or(&(0.0, 0.0)).0 as f64;
    }
}

fn main() -> Result<(), Box<dyn Error>> {

    let args = AppArgs::parse();

    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;


    //setup audio stream
    let host = cpal::default_host();
    let mut device = host.default_input_device().expect("No input device available");
    if args.device_name != "default" {
        device = host.input_devices().unwrap().into_iter()
        .find(|d|
            {
               d.name().unwrap().contains(&args.device_name)
            }
        ).unwrap();
    }
    
    let mut supported_configs_range = device.supported_input_configs().expect("error while querying configs");
    let supported_config = supported_configs_range.next().expect("no supported config?!")
    .with_max_sample_rate();
    let err_fn = |err| eprintln!("an error occurred on the output audio stream: {}", err);
    let sample_format = supported_config.sample_format();
    let config = supported_config.into();

    // list available devices and their properties
    if args.no_ui{
        println!("Devices: {:#?}", 
        host.devices().unwrap().into_iter().map(
            |d:Device|->String{
                format!("{} => {:?}",
                d.name().unwrap(), 
                d.supported_input_configs().unwrap().into_iter().collect::<Vec<SupportedStreamConfigRange>>())}
            )
            .collect::<Vec<String>>()
        );
        println!("Selected device: {}", device.name().unwrap());
    }

    //establish channel
    let mut snapshot_bus:Bus<[(f32, f32);SNAPSHOT_BUFFLEN]> =Bus::new(8);
    let pitch_snapshot_rx = snapshot_bus.add_rx();
    let wavviz_snapshot_rx = snapshot_bus.add_rx();
    // init timing vars
    let prev_time = Instant::now();
    let time = 0.0;
    //let audio_thread_snapshot_ref:Arc<Mutex<[(f32, f32); SNAPSHOT_BUFFLEN]>> = Arc::clone(&snapshot);
    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(&config, 
            closure!(move mut time, move mut prev_time, move mut snapshot_bus, |input:&[f32], _callbackdata| {
                
                let timediff = (Instant::now().duration_since(prev_time)).as_micros() as f32;

                let mut out:[(f32, f32);SNAPSHOT_BUFFLEN] = [(0.0,0.0); SNAPSHOT_BUFFLEN];
                for i in 0..input.len(){
                    let t = time + ((i+1) as f32 *timediff) as f32;
                    out[i] = (t, input[i]); // create tuple of timestamp with each sample
                } 
                snapshot_bus.broadcast(out);
                time += timediff;
                prev_time = Instant::now();
            }), 
            err_fn, None),
        sample_format => panic!("Unsupported sample format '{sample_format}'")
    }.unwrap();
    stream.play().unwrap(); // run in new thread


    // establish commincation lines to pitch estimator thread
   
    // run pitch estimator in new thread
    let mut f0_bus:Bus<(f32, f32, bool, f32)> = Bus::new(8);
    let freqviz_rx = f0_bus.add_rx();
    let midi_handler_rx = f0_bus.add_rx();

    let sr = args.srate.clone();
    let pthresh = args.power_thresh.clone();
    let cthresh = args.clairty_thresh.clone();
    let _pitch_thread_handle = thread::Builder::new().name("PitchDetectionThread".to_string())
    .spawn(closure!(move sr, move pthresh, move cthresh, move pitch_snapshot_rx, move mut f0_bus, ||{
        let mut detector = pitchdetect::PitchEstimator::new(sr, pitch_snapshot_rx, f0_bus, pthresh, cthresh);//TODO: query sample rate
        detector.run();
    })).unwrap();

    let _midi_thread_handle = thread::Builder::new().name("MidiHandlerThread".to_string())
    .spawn(||{
        let mut handler = midihandler::MidiHandler::new(midi_handler_rx);
        handler.run();
    }).unwrap();

    // create app and run it
    let tick_rate = Duration::from_millis(1);
    let app = App::new();
    let res = run_app(&mut terminal, app, tick_rate, wavviz_snapshot_rx, freqviz_rx, !args.no_ui);

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
    mut snapshot_rx: BusReader<[(f32, f32); SNAPSHOT_BUFFLEN]>,
    mut contour_rx: BusReader<(f32, f32, bool, f32)>,
    render_ui: bool
) -> io::Result<()> {
    let mut last_tick = Instant::now();
    
    loop {
        // wait for new audio frame
        app.waveform_snapshot = snapshot_rx.recv().unwrap();//_or([(0.0,0.0);SNAPSHOT_BUFFLEN]);

        // attempt to read new freq frame, if fail: use previous values
        let (timestamp, f0, voiced, _vprob) = contour_rx.recv().unwrap();//.unwrap_or((prev_timestamp, prev_f0, prev_voiced, 0.0f32));
        app.f0_contour.push((timestamp, if voiced {f0} else {0.0f32}));

        // render ui
        if render_ui {terminal.draw(|f| ui(f, &app))?;}

        // poll for quit event
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
            app.on_tick(); // update window bounds
            last_tick = Instant::now();
        }
    }
}

// generates ui
fn ui<B: Backend>(f: &mut Frame<B>, app: &App) {
    let size = f.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Ratio(1, 2),
                Constraint::Ratio(1, 2),
            ]
            .as_ref(),
        )
        .split(size);


    let wavviz_x_labels = vec![
        Span::styled(
            format!("{}", app.wavviz_window[0]),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("{}", (app.wavviz_window[0] + app.wavviz_window[1]) / 2.0)),
        Span::styled(
            format!("{}", app.wavviz_window[1]),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ];
    let wav_data = app.waveform_snapshot.iter().map(|&e| (e.0 as f64, e.1 as f64)).collect::<Vec::<(f64,f64)>>();
    let wav_datavec = vec![
        Dataset::default()
            .name("waveform_snapshot")
            .marker(symbols::Marker::Dot)
            .style(Style::default().fg(Color::Cyan))
            .data(wav_data.as_slice())
    ];
    let chart = Chart::new(wav_datavec)
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
                .labels(wavviz_x_labels)
                .bounds(app.wavviz_window),
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


    let mut f0_data = app.f0_contour.iter().map(|&e| (e.0 as f64, e.1 as f64)).collect::<Vec::<(f64,f64)>>().clone();
    f0_data.reverse();
    let f0_x_labels = vec![
        Span::styled(
            format!("{}", app.f0_window[0]),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("{}", (app.f0_window[0] + app.f0_window[1]) / 2.0)),
        Span::styled(
            format!("{}", app.f0_window[1]),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ];
    let f0_datavec = vec![
        Dataset::default()
            .name("f0_snapshot")
            .marker(symbols::Marker::Dot)
            .style(Style::default().fg(Color::LightMagenta))
            .data(f0_data.as_slice())
    ];
    let chart = Chart::new(f0_datavec)
        .block(
            Block::default()
                .title(Span::styled(
                    "f0",
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
                .labels(f0_x_labels)
                .bounds(app.f0_window)
        )
        .y_axis(
            Axis::default()
                .title("Frequency")
                .style(Style::default().fg(Color::Gray))
                .labels(vec![
                    Span::styled("0hz", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled("660hz", Style::default().add_modifier(Modifier::BOLD)),
                ])
                .bounds([0.0, 660.0]),
        );
    f.render_widget(chart, chunks[1]);
    
}
