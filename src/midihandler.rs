use midly::{Smf, live::LiveEvent, MidiMessage};
use midir::{MidiOutput, MidiOutputConnection, MidiOutputPort};
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

impl MidiHandler{
    pub fn new(f0_rx:Receiver<(f32, f32, bool, f32)>) -> MidiHandler{
        MidiHandler{
            freq_rx: f0_rx,
            curr_note: MidiNote{ 0, 0},
            voiced: false
        }
    }

    pub fn run(){
        // Your code here:
        /**You probably want something akin to:
         * Get freq from channel
         * Process freq data based on past freq data
         * Decide if note is being played
         * Send midi event over USB
         */
        loop{
            //WRITEME
        }
    }

}