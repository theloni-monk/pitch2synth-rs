use bus::BusReader;
use midir::MidiOutputConnection;
use midly::{ live::LiveEvent, MidiMessage };
use ringbuffer::{ AllocRingBuffer, RingBufferExt, RingBufferWrite };
use std::sync::Arc;
use std::sync::atomic::{ AtomicBool, Ordering };

use crate::get_midi_note;

const BUFFER_CAP: u8 = 8;

pub struct MidiHandlerThread {
    freq_rx: BusReader<(f32, f32, bool, f32)>,
    buffer: AllocRingBuffer<f32>,
    running: Arc<AtomicBool>,
}

fn note_swap(channel: u8, key: u8, on: bool) -> LiveEvent<'static> {
    let ev = midly::live::LiveEvent::Midi {
        channel: channel.into(),
        message: match on {
            true =>
                MidiMessage::NoteOn {
                    key: key.into(),
                    vel: (127).into(),
                },
            false =>
                MidiMessage::NoteOn {
                    key: key.into(),
                    vel: (0).into(),
                },
        },
    };
    ev
}

fn send_live_message(curr_note: &u8, last_note: u8, output: &mut MidiOutputConnection) {
    let mut live_buffer = Vec::new();

    note_swap(0, last_note, false).write(&mut live_buffer).unwrap();
    output.send(&live_buffer[..]).expect("Couldn't send MIDI message!");
    live_buffer.clear();

    note_swap(0, *curr_note, true).write(&mut live_buffer).unwrap();
    output.send(&live_buffer[..]).expect("Couldn't send MIDI message!");
}

impl MidiHandlerThread {
    pub fn new(f0_rx: BusReader<(f32, f32, bool, f32)>, running: Arc<AtomicBool>) -> MidiHandlerThread {
        MidiHandlerThread {
            freq_rx: f0_rx,
            buffer: AllocRingBuffer::with_capacity(BUFFER_CAP.into()),
            running: running,
        }
    }

    pub fn run(&mut self) {
        let midi_out = midir::MidiOutput::new("main").unwrap();
        if midi_out.port_count() < 1 {
            println!("couldn't find any midi outputs!");
            std::process::exit(0);
        }
        let main_port = &midi_out.ports()[midi_out.port_count() - 1]; //chooses the last midi device
        let port_name = midi_out.port_name(&main_port).expect("couldn't find port name!");

        let mut output_connection = midi_out
            .connect(&main_port, &port_name)
            .expect("couldn't establish connection");

        let mut last_note: u8 = 0;

        loop {
            if !self.running.load(Ordering::SeqCst) {
                break;
            }
            let (_timestamp, f0, _voiced, _vprob) = match self.freq_rx.recv() {
                Ok(data) => data,
                Err(_) => break
            };

            self.buffer.push(f0);

            let note = get_midi_note(self.buffer.iter().sum::<f32>() / (BUFFER_CAP as f32));
            if note != last_note {
                send_live_message(&note, last_note, &mut output_connection);
                last_note = note;
            }
        }
    }
}
