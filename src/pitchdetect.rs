use bus::{Bus, BusReader};
use ringbuffer::{AllocRingBuffer, RingBufferWrite};

use crate::MIN_FREQ;
use crate::NUM_FREQS;
use crate::SNAPSHOT_BUFFLEN;
mod goertzel;

const NUM_FRAMES_CONCAT: usize = 32;

pub struct PitchEstimatorThread {
    audio_rx: BusReader<[(f32, f32); SNAPSHOT_BUFFLEN]>,
    pitch_tx: Bus<(f32, f32, bool, f32)>, //sends (frequency float in hz, voiced bool, voiced probability float)
    spec_tx: Bus<[f32; NUM_FREQS]>,
    waveform_snapshot_buffer: AllocRingBuffer<[(f32, f32); SNAPSHOT_BUFFLEN]>,
    predictor: goertzel::GoertzelEstimator,
    cthresh: f32,
}

impl PitchEstimatorThread {
    pub fn new(
        sr: usize,
        snapshot_ref: BusReader<[(f32, f32); SNAPSHOT_BUFFLEN]>,
        f0_tx: Bus<(f32, f32, bool, f32)>,
        spec_tx: Bus<[f32; NUM_FREQS]>,
        cthresh: f32,
    ) -> PitchEstimatorThread {
        let detector = goertzel::GoertzelEstimator::new(MIN_FREQ, sr as f32);
        let mut ringbuff = AllocRingBuffer::with_capacity(NUM_FRAMES_CONCAT);
        ringbuff.push([(0.0, 0.0); SNAPSHOT_BUFFLEN]);
        PitchEstimatorThread {
            audio_rx: snapshot_ref,
            pitch_tx: f0_tx,
            spec_tx: spec_tx,
            waveform_snapshot_buffer: ringbuff,
            predictor: detector,
            cthresh: cthresh,
        }
    }
    pub fn run(&mut self) {
        let mut multi_frame_snapshot = [(0.0, 0.0); SNAPSHOT_BUFFLEN * NUM_FRAMES_CONCAT];

        loop {
            self.waveform_snapshot_buffer
                .push(self.audio_rx.recv().unwrap());

            for i in 0..NUM_FRAMES_CONCAT {
                for j in 0..SNAPSHOT_BUFFLEN {
                    multi_frame_snapshot[i * SNAPSHOT_BUFFLEN + j] =
                        self.waveform_snapshot_buffer[i as isize][j];
                }
            }

            let timestamp = multi_frame_snapshot[0].0;
            let amps = multi_frame_snapshot
                .iter()
                .map(|el| el.1)
                .collect::<Vec<f32>>();
            self.predictor.process(amps.as_slice());
            let pitch = self.predictor.get_pitch();

            self.spec_tx.broadcast(self.predictor.gvec);
            self.pitch_tx
                .broadcast((timestamp, pitch.0, pitch.1 > self.cthresh, pitch.1));
        }
    }
}
