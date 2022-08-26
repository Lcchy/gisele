use anyhow::Result;
use jack::{Client, ClientOptions, RawMidi};
use rand::Rng;
use std::{
    cmp::min,
    io,
    sync::{Arc, RwLock},
};

struct MidiNote {
    on_off: bool,
    /// Channel, should be 0-15
    channel: u8,
    pitch: u8,
    velocity: u8,
    // / usec
    // len: u64,
}

impl MidiNote {
    fn get_raw_note_on_bytes(&self) -> [u8; 3] {
        [
            (8 + self.on_off as u8) * 16 + self.channel,
            self.pitch,
            self.velocity,
        ]
    }

    // fn get_raw_note_on<'a, 'b>(&'a self, cycle_frames: u32) -> RawMidi<'b>
    // where
    //     'a: 'b,
    // {
    //     RawMidi {
    //         time: cycle_frames,
    //         bytes: &[9 * 16 + self.channel, self.pitch, self.velocity],
    //         // bytes: &self.get_raw_note_on_bytes().to_owned(),
    //     }
    // }
}

struct Event {
    e_type: EventType,
    /// usec event position from start position
    time: u64,
}

enum EventType {
    MidiNote(MidiNote),
}

type EventBuffer = Vec<Event>;

struct SeqParams {
    bpm: u16,
    /// In usecs
    loop_length: u64,
    nb_events: u64,
    // density
}

#[derive(Clone)]
struct Params {
    /// Current position in the event buffer.
    /// Write: jack process, Read: -
    event_head: usize,
    /// In usecs. To be reset on loop or start/stop
    /// Write: jack process, Read: -
    curr_time_start: u64,
    /// In usecs. To be reset on loop or start/stop
    /// Write: jack process, Read: -
    curr_time_end: u64,
    /// Write: osc process, Read: Jack process
    seq_params: Arc<RwLock<SeqParams>>,
    /// Events should be ordered by their times
    /// Write: osc process, Read: Jack process
    event_buffer: Arc<RwLock<EventBuffer>>,
}

fn main() -> Result<()> {
    // Set up jack ports
    let (jclient, _) = Client::new("gisele_jack", ClientOptions::NO_START_SERVER)?;

    let mut out_port = jclient
        .register_port("gisele_out", jack::MidiOut::default())
        .unwrap();

    // TODO LATER: have a central sequencer process that pushes out events to jack midi or osc sender

    // Init values
    //TODO proper density input function
    // // Should be 0<=..<1
    // let event_density = 0.3f64;
    // // Capping at 1 event every 10 us
    // let nb_events = min(
    //     -(1. - event_density).ln(),
    //     loop_length_arc.as_ref().read().unwrap().checked_div(10.),
    // );
    let nb_events = 10;
    let loop_length = 2_000_000; //2sec = 4 bars at 120 bpm
    let mut event_buffer = gen_rand_midi_vec(loop_length, nb_events);
    event_buffer.sort_by_key(|e| e.time);
    let params_arc = Params {
        event_head: 0,
        curr_time_start: 0,
        curr_time_end: 0,
        event_buffer: Arc::new(RwLock::new(event_buffer)),
        seq_params: Arc::new(RwLock::new(SeqParams {
            bpm: 120,
            loop_length, //2sec = 4 bars at 120 bpm
            nb_events,
        })),
    };
    let mut params_ref = params_arc;

    // Define the Jack process
    let jack_process = move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        // Max event buff size was measured as 32736
        let mut out_buff = out_port.writer(ps);

        let loop_len = params_ref.seq_params.read().unwrap().loop_length;
        let event_buffer = params_ref.event_buffer.read().unwrap();

        let cy_times = ps.cycle_times().unwrap();
        params_ref.curr_time_end =
            (params_ref.curr_time_end + (cy_times.next_usecs - cy_times.current_usecs)) % loop_len;

        // println!("next_event.time {}", next_event.time);
        // println!("Curr time {}", params_ref.curr_time_end);
        // println!("Curr frames {}", cy_times.current_frames);
        // println!("frames sunce start {}", ps.frames_since_cycle_start());
        // println!("frames sunce start {}", ps.frames_since_cycle_start());

        loop {
            let next_event = &event_buffer[params_ref.event_head];
            // This shitty check should be removed once we map events to frames directly
            let push_event = if params_ref.curr_time_start < params_ref.curr_time_end {
                params_ref.curr_time_start <= next_event.time
                    && next_event.time < params_ref.curr_time_end
            } else {
                // Wrapping case
                println!("LOOPING");
                println!("  Curr start time {}", params_ref.curr_time_start);
                println!("  Curr end time {}", params_ref.curr_time_end);
                params_ref.curr_time_start <= next_event.time
                    || next_event.time < params_ref.curr_time_end
            };

            if push_event {
                match next_event.e_type {
                    EventType::MidiNote(ref note) => {
                        let raw_midi = RawMidi {
                            //TODO add some frames here for precise timing, as a process cycle is 42ms, see jack doc
                            // This should allow to map events on specific frames, making the above if condition redundant
                            time: ps.frames_since_cycle_start(),
                            // bytes: &[144, 60, 64],
                            bytes: &note.get_raw_note_on_bytes(),
                        };
                        // println!("{:?}", note.get_raw_note_on_bytes());
                        out_buff.write(&raw_midi).unwrap();
                        println!("Sending note {:?}", &note.get_raw_note_on_bytes());
                        println!("  Curr start time {}", params_ref.curr_time_start);
                        println!("  Event time {:?}", next_event.time);
                        println!("  Curr end time {}", params_ref.curr_time_end);
                    }
                }
                params_ref.event_head = (params_ref.event_head + 1) % event_buffer.len();
            } else {
                break;
            }
        }

        params_ref.curr_time_start = params_ref.curr_time_end;
        // println!("frames sunce start {}", ps.frames_since_cycle_start());

        jack::Control::Continue
    };

    // Start the Jack thread
    let process = jack::ClosureProcessHandler::new(jack_process);
    let active_client = jclient.activate_async((), process).unwrap();

    // Wait for user input to quit
    println!("Press enter/return to quit...");
    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input).ok();
    active_client.deactivate().unwrap();

    Ok(())
}

fn gen_rand_midi_vec(loop_len: u64, nb_events: u64) -> Vec<Event> {
    let mut rng = rand::thread_rng();
    let mut events_buffer = vec![];

    for _ in 0..nb_events {
        let velocity = rng.gen_range(0..127);
        let pitch = rng.gen_range(0..127);
        let time_offset = rng.gen_range(0..loop_len);
        let note_len = rng.gen_range(0..1_000_000);
        let event_midi_on = Event {
            e_type: EventType::MidiNote(MidiNote {
                channel: 0,
                pitch,
                velocity,
                on_off: true,
            }),
            time: time_offset,
        };
        let event_midi_off = Event {
            e_type: EventType::MidiNote(MidiNote {
                channel: 0,
                pitch,
                velocity,
                on_off: false,
            }),
            time: min(time_offset + note_len, loop_len),
        };
        events_buffer.push(event_midi_on);
        events_buffer.push(event_midi_off);
    }
    events_buffer
}
