use anyhow::Result;
use jack::{Client, ClientOptions, RawMidi};
use midi::MidiNote;
use std::{
    io,
    sync::{Arc, RwLock},
};

use crate::midi::gen_rand_midi_vec;

mod midi;

pub struct Event {
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
    /// In usecs,//TODO to be quantized to whole note on bpm, with option to deviate
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
    j_window_time_start: u64,
    /// In usecs. To be reset on loop or start/stop
    /// Write: jack process, Read: -
    j_window_time_end: u64,
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
    let nb_events = 100;
    let loop_length = 20_000_000; //2sec = 4 bars at 120 bpm
    let bpm = 120;
    let mut event_buffer = gen_rand_midi_vec(bpm, loop_length, nb_events);
    event_buffer.sort_by_key(|e| e.time);
    let params_arc = Params {
        event_head: 0,
        j_window_time_start: 0,
        j_window_time_end: 0,
        event_buffer: Arc::new(RwLock::new(event_buffer)),
        seq_params: Arc::new(RwLock::new(SeqParams {
            bpm,
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
        params_ref.j_window_time_end = (params_ref.j_window_time_end
            + (cy_times.next_usecs - cy_times.current_usecs))
            % loop_len;

        // println!("next_event.time {}", next_event.time);
        // println!("Curr time {}", params_ref.curr_time_end);
        // println!("Curr frames {}", cy_times.current_frames);
        // println!("frames sunce start {}", ps.frames_since_cycle_start());
        // println!("frames sunce start {}", ps.frames_since_cycle_start());

        loop {
            let next_event = &event_buffer[params_ref.event_head];
            // This shitty check should be removed once we map events to frames directly
            let push_event = if params_ref.j_window_time_start < params_ref.j_window_time_end {
                params_ref.j_window_time_start <= next_event.time
                    && next_event.time < params_ref.j_window_time_end
            } else {
                // Wrapping case
                println!("LOOPING");
                // println!("start {}", params_ref.curr_time_start);
                // println!("event {}", next_event.time);
                // println!("end {}", params_ref.curr_time_end);
                params_ref.j_window_time_start <= next_event.time
                    || next_event.time < params_ref.j_window_time_end
            };

            if push_event {
                match next_event.e_type {
                    EventType::MidiNote(ref note) => {
                        let raw_midi = RawMidi {
                            //TODO add some frames here for precise timing, as a process cycle is 42ms, see jack doc
                            // This should allow to map events on specific frames, making the above if condition redundant
                            time: ps.frames_since_cycle_start(),
                            bytes: &note.get_raw_note_on_bytes(),
                        };
                        out_buff.write(&raw_midi).unwrap();
                        println!(
                            "Sending midi note: Channel {:<5} Pitch {:<5} Vel {:<5} On/Off {:<5}",
                            note.channel, note.pitch, note.velocity, note.on_off
                        );
                    }
                }
                params_ref.event_head = (params_ref.event_head + 1) % event_buffer.len();
            } else {
                break;
            }
        }

        params_ref.j_window_time_start = params_ref.j_window_time_end;
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
