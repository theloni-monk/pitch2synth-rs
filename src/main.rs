use std::{ error::Error, io::{self, Write}, thread, time::{ Duration, Instant }, iter::zip };
use crossterm::{
    event::{ self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode },
    execute,
    terminal::{ disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen },
};
use tui::{
    backend::{ Backend, CrosstermBackend },
    layout::{ Constraint, Direction, Layout },
    style::{ Color, Modifier, Style },
    symbols,
    text::Span,
    widgets::{ Axis, Block, Borders, Chart, Dataset, BarChart },
    Frame,
    Terminal,
};
use clap::Parser;
use cpal::{
    traits::{ HostTrait, DeviceTrait, StreamTrait },
    SampleFormat,
    Device,
    SupportedStreamConfigRange,
};
use closure::closure;
use ringbuffer::{ AllocRingBuffer, RingBufferWrite, RingBufferExt };
use bus::{ Bus, BusReader };

mod pitchdetect;
mod midihandler;
//FIXME: allow for oversized buffer
const SNAPSHOT_BUFFLEN: usize = 1024; //882;
const CONTOUR_BUFFLEN: usize = 128;

const MIN_FREQ: f32 = 15.434; //B0
const A4: f32 = 440.0;
const NUM_FREQS: usize = 96;
const NOISE_THRESH: f32 = 100.0;

const BIN_LABELS: [&'static str; NUM_FREQS] = ["_"; NUM_FREQS];
const NOTE_LABELS: [&'static str; 12] = [
    "C",
    "C#",
    "D",
    "Eb",
    "E",
    "F",
    "F#",
    "G",
    "Ab",
    "A",
    "Bb",
    "B",
];

fn get_midi_note(frequency: f32) -> u8 {
    let semitone = 12.0 * f32::log2(frequency / A4) + 69.0;
    semitone.round() as u8
}

fn get_freq(midi_note: u8) -> f32 {
    let semitone = (midi_note as f32) + 1.0; //MIN_FREQ is B1 not C1 so we compensate
    let freq = MIN_FREQ * (2.0f32).powf(semitone / 12.0 - 1.0);
    return freq;
}

fn get_note_label(freq: f32) -> &'static str {
    let mut midi_idx = get_midi_note(freq);
    midi_idx = midi_idx % 12;
    return NOTE_LABELS[midi_idx as usize];
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct AppArgs {
    #[arg(short, long, default_value_t = { "default".to_string() })]
    device_name: String,

    #[arg(short, long, default_value_t = 48000)]
    srate: usize,

    #[arg(short, long, default_value_t = 0.2)]
    clairty_thresh: f32,

    #[arg(short, long, default_value_t = false)]
    no_ui: bool,
}

struct App<'a> {
    waveform_snapshot: [(f32, f32); SNAPSHOT_BUFFLEN],
    f0_contour: AllocRingBuffer<(f32, f32)>,
    spectrogram: Vec<(&'a str, f32)>,
    wavviz_window: [f64; 2],
    f0_window: [f64; 2],
}

impl<'a> App<'a> {
    fn new() -> App<'a> {
        let str_freq_arr = BIN_LABELS.into_iter();
        let init_freq_amps = [0.0f32; NUM_FREQS];
        App {
            waveform_snapshot: [(0.0, 0.0); SNAPSHOT_BUFFLEN],
            wavviz_window: [0.0, 63555000.0],
            f0_contour: AllocRingBuffer::with_capacity(CONTOUR_BUFFLEN),
            spectrogram: zip(str_freq_arr, init_freq_amps).collect::<Vec<(&str, f32)>>(),
            f0_window: [0.0, 63555000.0],
        }
    }

    //update window bounds
    fn on_tick(&mut self) {
        self.wavviz_window[0] = self.waveform_snapshot[0].0 as f64;
        self.wavviz_window[1] = self.waveform_snapshot[SNAPSHOT_BUFFLEN - 1].0 as f64;
        self.f0_window[0] = self.f0_contour.get(0).unwrap_or(&(0.0, 0.0)).0 as f64;
        self.f0_window[1] = self.f0_contour.get(-1).unwrap_or(&(0.0, 0.0)).0 as f64;
    }
}

fn select_device_and_config() -> Result<(Device, cpal::SupportedStreamConfig), Box<dyn Error>> {
    // setup audio stream - interactive device & config selection
    let host = cpal::default_host();

    // gather input devices
    let devices = host
        .input_devices()
        .expect("No input devices available")
        .into_iter()
        .collect::<Vec<Device>>();

    println!("Available input devices:");
    for (i, d) in devices.iter().enumerate() {
        println!("  [{}] {}", i, d.name().unwrap_or("<Unknown>".to_string()));
    }

    // prompt user to select device (press Enter to choose default)
    print!("Select device index (press Enter for default device): ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let selection = input.trim();

    let device: Device = if selection.is_empty() {
        host.default_input_device().expect("No default input device available")
    } else {
        let idx: usize = selection.parse().expect("Invalid device index");
        devices.into_iter().nth(idx).expect("Device index out of range")
    };

    println!("Selected device: {}", device.name().unwrap_or("<Unknown>".to_string()));

    // list supported configs for chosen device
    let configs = device
        .supported_input_configs()
        .expect("error while querying configs")
        .into_iter()
        .collect::<Vec<SupportedStreamConfigRange>>();

    println!("Supported input configs for '{}':", device.name().unwrap_or("<Unknown>".to_string()));
    for (i, c) in configs.iter().enumerate() {
        println!(
            "  [{}] {:?}, channels: {}, min_rate: {}, max_rate: {}, buffer_size: {:?}",
            i,
            c.sample_format(),
            c.channels(),
            c.min_sample_rate().0,
            c.max_sample_rate().0,
            c.buffer_size()
        );
    }

    // prompt user to select config (press Enter to choose first config)
    print!("Select config index (press Enter for first config): ");
    io::stdout().flush()?;
    input.clear();
    io::stdin().read_line(&mut input)?;
    let selection = input.trim();

    let supported_config = if selection.is_empty() {
        configs.into_iter().nth(0).expect("no supported config?!").with_max_sample_rate()
    } else {
        let idx: usize = selection.parse().expect("Invalid config index");
        configs.into_iter().nth(idx).expect("config index out of range").with_max_sample_rate()
    };

    Ok((device, supported_config))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = AppArgs::parse();

    let (device, supported_config) = select_device_and_config()?;

    let err_fn = |err| eprintln!("an error occurred on the output audio stream: {}", err);
    let sample_format = supported_config.sample_format();
    let config = supported_config.into();

    //establish channel
    let mut snapshot_bus: Bus<[(f32, f32); SNAPSHOT_BUFFLEN]> = Bus::new(8);
    let pitch_snapshot_rx = snapshot_bus.add_rx();
    let wavviz_snapshot_rx = snapshot_bus.add_rx();
    // init timing vars
    let prev_time = Instant::now();
    let time = 0.0;
    //let audio_thread_snapshot_ref:Arc<Mutex<[(f32, f32); SNAPSHOT_BUFFLEN]>> = Arc::clone(&snapshot);
    let stream = (
        match sample_format {
            SampleFormat::F32 =>
                device.build_input_stream(
                    &config,
                    closure!(move mut time, move mut prev_time, move mut snapshot_bus, |input:&[f32], _callbackdata| {
                //FIXME: detect multiple channels interleaved
                let timediff = (Instant::now().duration_since(prev_time)).as_micros() as f32;

                let mut out:[(f32, f32);SNAPSHOT_BUFFLEN] = [(0.0,0.0); SNAPSHOT_BUFFLEN];
                let max_idx = std::cmp::min(SNAPSHOT_BUFFLEN, input.len());
                for i in 0..max_idx - 1 {
                    let t = time + ((i+1) as f32 *timediff) as f32;
                    out[i] = (t, input[i]); // create tuple of timestamp with each sample
                } 
                snapshot_bus.broadcast(out);
                time += timediff;
                prev_time = Instant::now();
            }),
                    err_fn,
                    None
                ),
            sample_format => panic!("Unsupported sample format '{sample_format}'"),
        }
    ).unwrap();
    stream.play().unwrap(); // run in new thread

    // establish commincation lines to pitch estimator thread
    let mut f0_bus: Bus<(f32, f32, bool, f32)> = Bus::new(8);
    let freqviz_rx = f0_bus.add_rx();
    let midi_handler_rx = f0_bus.add_rx();
    let mut spectrogram_bus: Bus<[f32; NUM_FREQS]> = Bus::new(8);
    let spectrogram_rx = spectrogram_bus.add_rx();

    let sr = args.srate.clone();
    let cthresh = args.clairty_thresh.clone();
    let pitch_thread_handle = thread::Builder
        ::new()
        .name("PitchDetectionThread".to_string())
        .spawn(
            closure!(move sr,  move cthresh, move pitch_snapshot_rx, move mut f0_bus, move mut spectrogram_bus, || {
                let mut detector = pitchdetect::PitchEstimatorThread::new(sr, pitch_snapshot_rx, f0_bus, spectrogram_bus, cthresh);
                detector.run();
            })
        )
        .unwrap();

    let _midi_thread_handle = thread::Builder
        ::new()
        .name("MidiHandlerThread".to_string())
        .spawn(|| {
            let mut handler = midihandler::MidiHandlerThread::new(midi_handler_rx);
            handler.run();
        })
        .unwrap();

    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let tick_rate = Duration::from_millis(1);
    let app = App::new();
    let res = run_app(
        &mut terminal,
        app,
        tick_rate,
        wavviz_snapshot_rx,
        freqviz_rx,
        spectrogram_rx,
        !args.no_ui
    );

    stream.pause().unwrap();

    // restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err);
    }

    Ok(())
}

fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
    tick_rate: Duration,
    mut snapshot_rx: BusReader<[(f32, f32); SNAPSHOT_BUFFLEN]>,
    mut contour_rx: BusReader<(f32, f32, bool, f32)>,
    mut spectrogram_rx: BusReader<[f32; NUM_FREQS]>,
    render_ui: bool
) -> io::Result<()> {
    let mut last_tick = Instant::now();

    loop {
        // wait for new audio frame
        app.waveform_snapshot = snapshot_rx.recv().unwrap();

        // attempt to read new freq frame, if fail: use previous values
        let (timestamp, f0, voiced, _vprob) = contour_rx.recv().unwrap();
        app.f0_contour.push((timestamp, if voiced { f0 } else { 0.0f32 }));

        let specdata = spectrogram_rx.recv().unwrap();
        for i in 0..NUM_FREQS {
            app.spectrogram[i].1 = specdata[i];
        }

        // render ui
        if render_ui {
            terminal.draw(|f| ui(f, &app))?;
        }

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
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(size);

    let mut bardata_float = app.spectrogram.clone();
    // supress noise, display all zeros if total strength is less than a tenth of the noise thresh
    if
        bardata_float
            .iter()
            .map(|el| el.1)
            .sum::<f32>() < (NOISE_THRESH * (NUM_FREQS as f32)) / 25.0
    {
        for i in 0..NUM_FREQS {
            bardata_float[i].1 = 0.0;
        }
    }
    let bardata_u64: Vec<(&str, u64)> = bardata_float
        .into_iter()
        .map(|(s, f)| (s, f as u64))
        .collect();
    let barchart = BarChart::default()
        .block(Block::default().title("Spectrogram").borders(Borders::ALL))
        .data(bardata_u64.as_slice())
        .bar_width(1)
        .bar_gap(1)
        .bar_style(Style::default().fg(Color::White))
        .value_style(Style::default().bg(Color::White).add_modifier(Modifier::BOLD));

    f.render_widget(barchart, chunks[0]);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(chunks[1]);

    let wavviz_x_labels = vec![
        Span::styled(
            format!("{}", app.wavviz_window[0]),
            Style::default().add_modifier(Modifier::BOLD)
        ),
        Span::raw(format!("{}", (app.wavviz_window[0] + app.wavviz_window[1]) / 2.0)),
        Span::styled(
            format!("{}", app.wavviz_window[1]),
            Style::default().add_modifier(Modifier::BOLD)
        )
    ];
    let wav_data = app.waveform_snapshot
        .iter()
        .map(|&e| (e.0 as f64, e.1 as f64))
        .collect::<Vec<(f64, f64)>>();
    let wav_datavec = vec![
        Dataset::default()
            .name("waveform_snapshot")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Cyan))
            .data(wav_data.as_slice())
    ];
    let chart = Chart::new(wav_datavec)
        .block(
            Block::default()
                .title(
                    Span::styled(
                        "waveform",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    )
                )
                .borders(Borders::ALL)
        )
        .x_axis(
            Axis::default()
                .title("Timestep")
                .style(Style::default().fg(Color::Gray))
                .labels(wavviz_x_labels)
                .bounds(app.wavviz_window)
        )
        .y_axis(
            Axis::default()
                .title("Amplitude")
                .style(Style::default().fg(Color::Gray))
                .labels(
                    vec![
                        Span::styled("-1", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw("0"),
                        Span::styled("1", Style::default().add_modifier(Modifier::BOLD))
                    ]
                )
                .bounds([-1.0, 1.0])
        );
    f.render_widget(chart, chunks[0]);

    let mut f0_data = app.f0_contour
        .iter()
        .map(|&e| (e.0 as f64, e.1 as f64))
        .collect::<Vec<(f64, f64)>>()
        .clone();
    f0_data.reverse();
    let f0_x_labels = vec![
        Span::styled(
            format!("{}", app.f0_window[0]),
            Style::default().add_modifier(Modifier::BOLD)
        ),
        Span::raw(format!("{}", (app.f0_window[0] + app.f0_window[1]) / 2.0)),
        Span::styled(format!("{}", app.f0_window[1]), Style::default().add_modifier(Modifier::BOLD))
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
                .title(
                    Span::styled(
                        get_note_label(f0_data[0].1 as f32),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    )
                )
                .borders(Borders::ALL)
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
                .labels(
                    vec![
                        Span::styled("0hz", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled("200hz", Style::default().add_modifier(Modifier::BOLD))
                    ]
                )
                .bounds([0.0, 200.0])
        );
    f.render_widget(chart, chunks[1]);
}
