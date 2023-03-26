use std::time::{Instant, Duration};
use bus::BusReader;
use midly::{Smf, live::LiveEvent, MidiMessage, Header, Format, Timing, num::{u4, u7}, TrackEventKind};
use midir::{MidiOutput, MidiOutputConnection, MidiOutputPort};
use ringbuffer::{AllocRingBuffer, RingBufferWrite, RingBufferExt};

const A4: f32 = 440.0;
const BUFFER_CAP: u8 = 16;

pub struct MidiHandler{
    freq_rx: BusReader<(f32, f32, bool, f32)>,
    buffer: AllocRingBuffer<f32>,
    voiced: bool
}

fn get_midi_note(frequency: f32) -> u8 {
    let semitone = 12.0 * f32::log2(frequency / A4) + 69.0;
    semitone.round() as u8 
}

fn note_swap(channel: u8, key: u8, on: bool) -> TrackEventKind<'static>{
    let ev = midly::TrackEventKind::Midi{
        channel: channel.into(),
        message: match on {
            true => MidiMessage::NoteOn {
                key: key.into(),
                vel: 127.into(),
            },
            false => MidiMessage::NoteOff {
                key: key.into(),
                vel: 127.into(),
            }
        }
    };
    ev
}

impl MidiHandler{
    pub fn new(f0_rx:BusReader<(f32, f32, bool, f32)>) -> MidiHandler{
        MidiHandler{
            freq_rx: f0_rx,
            voiced: false,
            buffer: AllocRingBuffer::with_capacity(BUFFER_CAP.into())
        }
    }


    pub fn run(&mut self){
        // Your code here:
        /* You probably want something akin to:
         * Get freq from channel
         * Process freq data based on past freq data
         * Decide if note is being played
         * Send midi event over USB
         */
        
        let mut prev_poll = Instant::now();
        let mut last_event = Instant::now();

        let mut last_note: u8 = 0;
        let mut smf = Smf::new(Header {
                                format: Format::SingleTrack,
                                timing: Timing::Timecode(midly::Fps::Fps25, 40) 
                            });
        
        smf.tracks.push(Vec::new());

        //implement Live event messaging
        loop{
                let (_timestamp, f0, _voiced, _vprob) = self.freq_rx.recv().unwrap();
                self.buffer.push(f0);
                let note = get_midi_note(self.buffer.iter().sum::<f32>() / BUFFER_CAP as f32);
                if note != last_note {
                    let diff = Instant::now().duration_since(last_event).as_millis();

                    smf.tracks[0].push(midly::TrackEvent {delta: (diff as u32).into(), kind: note_swap(0, last_note, false)});
                    smf.tracks[0].push(midly::TrackEvent {delta: 0.into(), kind: note_swap(0, note, true)});

                    // dbg!(note);

                    last_note = note;
                    last_event = Instant::now();
                }
                
                if smf.tracks[0].len() > 75 {
                    break
                }
        }

        let end_diff = Instant::now().duration_since(last_event).as_millis();
        smf.tracks[0].push(midly::TrackEvent {delta: (end_diff as u32).into(), kind: midly::TrackEventKind::Meta(midly::MetaMessage::EndOfTrack)});
        println!("{:?}", smf.tracks);

    }

}