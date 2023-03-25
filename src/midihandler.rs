use std::time::{Instant, Duration};

use midly::{Smf, live::LiveEvent, MidiMessage};
use midir::{MidiOutput, MidiOutputConnection, MidiOutputPort};
use  spmc::{Receiver};

const C0: f32 = 16.3515978313;
const POLL_TIME:u64 = 50; //ms

struct MidiNote{
    note: u8, // midi byte encoding of a note value
    vel: u8 // how strongly the note is played
}
// feel free to modify or add any fields that may be useful
pub struct MidiHandler{
    freq_rx: Receiver<(f32, f32, bool, f32)>,
    curr_note: MidiNote,
    voiced: bool
}

fn get_midi_note(frequency: &f32) -> u8 {
    let semitone = 12.0 * f32::log10(frequency / C0) / f32::log10(2.0);
    // dbg!(semitone);
    let octave = (&semitone / 12.0).round(); 
    let note = semitone - 12.0 * octave;
    note as u8
}

impl MidiHandler{
    pub fn new(f0_rx:Receiver<(f32, f32, bool, f32)>) -> MidiHandler{
        MidiHandler{
            freq_rx: f0_rx,
            curr_note: MidiNote{ note:0, vel:0},
            voiced: false
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
        //TOdo; poll every 50ms
        let mut prev_poll = Instant::now();

        let mut prev_timestamp = 0.0f32;
        let mut prev_f0 = 0.0f32;
        let mut prev_voiced = false;
        
        loop{
            let poll_time = Instant::now().duration_since(prev_poll);
            if poll_time>Duration::from_millis(POLL_TIME){
                let (timestamp, f0, voiced, _vprob) = self.freq_rx.recv().unwrap();
                if prev_f0 != f0 && voiced {
                    println!("{:?}", get_midi_note(&f0));
                }
                prev_f0 = f0;
                prev_voiced = voiced;
                prev_poll = Instant::now();
            }
            
            // println!("{:#?}", self.freq_rx.recv().unwrap());
            // std::process::exit(0);
        }
    }

}