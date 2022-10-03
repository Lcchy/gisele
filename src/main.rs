use anyhow::Result;
use jack::{Client, ClientOptions, RawMidi};
use midi::gen_rand_midi_vec;
use osc::{osc_process_closure, OSC_PORT};
use seq::SeqParams;
use seq::{Event, EventType, SeqInternal, SeqInternalStatus, SeqStatus};
use std::{
    io,
    net::UdpSocket,
    sync::{Arc, RwLock},
    thread,
};

mod midi;
mod osc;
mod seq;

pub struct Params {
    /// Write: osc process, Read: Jack process
    seq_params: Arc<RwLock<SeqParams>>,
    /// Event Bufffer
    /// Events should be ordered by their times
    /// Write: TBD, Read: Jack process
    event_buffer: Arc<RwLock<Vec<Event>>>,
}

fn main() -> Result<()> {
    // Set up jack ports
    let (jclient, _) = Client::new("gisele_jack", ClientOptions::NO_START_SERVER)?;

    let mut out_port = jclient
        .register_port("gisele_out", jack::MidiOut::default())
        .unwrap();

    // Init values
    let seq_params = SeqParams {
        status: SeqStatus::Stop,
        bpm: 120,
        loop_length: 20_000_000, //2sec = 4 bars at 120 bpm
        nb_events: 100,
    };
    let event_buffer =
        gen_rand_midi_vec(seq_params.bpm, seq_params.loop_length, seq_params.nb_events);
    let params_arc = Arc::new(Params {
        event_buffer: Arc::new(RwLock::new(event_buffer)),
        seq_params: Arc::new(RwLock::new(seq_params)),
    });
    let params_ref = params_arc.clone();
    let mut seq_int = SeqInternal::new();

    // Define the Jack process
    let jack_process = move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        let seq_params = params_ref.seq_params.read().unwrap();

        // Handle Sequencer statuses
        if seq_params.status == SeqStatus::Start {
            seq_int.status = SeqInternalStatus::Playing;
        }
        if seq_int.status == SeqInternalStatus::Silence {
            return jack::Control::Continue;
        }

        let event_buffer = &*params_ref.event_buffer.read().unwrap();
        let mut out_buff = out_port.writer(ps);
        let loop_len = seq_params.loop_length;
        let cy_times = ps.cycle_times().unwrap();

        seq_int.j_window_time_end =
            (seq_int.j_window_time_end + (cy_times.next_usecs - cy_times.current_usecs)) % loop_len;

        // println!("next_event.time {}", next_event.time);
        // println!("Curr time {}", params_ref.curr_time_end);
        // println!("Curr frames {}", cy_times.current_frames);
        // println!("frames sunce start {}", ps.frames_since_cycle_start());

        let event_head_before = seq_int.event_head;
        let halting = seq_params.status == SeqStatus::Pause || seq_params.status == SeqStatus::Stop;
        loop {
            let next_event = &event_buffer[seq_int.event_head];
            let mut push_event = seq_int.event_in_cycle(next_event.time);

            // We let the seq play once through all midi off notes when halting.
            let mut jump_event = false;
            if halting {
                if let EventType::MidiNote(n) = next_event.e_type {
                    if !n.on_off {
                        // Let through all midi off msgs for Pause and stop.
                        push_event = true;
                    } else if (seq_int.event_head + 1) % event_buffer.len() != event_head_before {
                        jump_event = true;
                    }
                }
            };

            if push_event {
                match next_event.e_type {
                    EventType::MidiNote(ref note) => {
                        let raw_midi = RawMidi {
                            time: ps.frames_since_cycle_start(),
                            bytes: &note.get_raw_note_on_bytes(),
                        };
                        // Max event buff size was measured at ~32kbits ? In practice, 800-2200 midi msgs
                        out_buff.write(&raw_midi).unwrap();
                        println!(
                            "Sending midi note: Channel {:<5} Pitch {:<5} Vel {:<5} On/Off {:<5}",
                            note.channel, note.pitch, note.velocity, note.on_off
                        );
                    }
                }
                seq_int.event_head = (seq_int.event_head + 1) % event_buffer.len();
            } else if jump_event {
                seq_int.event_head = (seq_int.event_head + 1) % event_buffer.len();
                continue;
            } else {
                break;
            }

            // Stop playing when halting after having played all notes off (one full loop)
            if halting && seq_int.event_head == event_head_before {
                break;
            }
        }

        // Reset the seq to start or current position in case of a stop or pause
        if seq_params.status == SeqStatus::Pause || seq_params.status == SeqStatus::Stop {
            seq_int.event_head = event_head_before;
            seq_int.status = SeqInternalStatus::Silence;
        }
        if seq_params.status == SeqStatus::Stop {
            seq_int.stop_reset();
        }

        seq_int.j_window_time_start = seq_int.j_window_time_end;

        jack::Control::Continue
    };

    // Start the Jack thread
    let process = jack::ClosureProcessHandler::new(jack_process);
    let active_client = jclient.activate_async((), process).unwrap();

    // Start the OSC listening thread
    let udp_socket = UdpSocket::bind(format!("127.0.0.1:{}", OSC_PORT))?;
    let osc_process = osc_process_closure(udp_socket, params_arc);
    let osc_handler = thread::spawn(osc_process);

    // Wait for user input to quit
    println!("Press enter/return to quit...");
    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input).ok();
    active_client.deactivate().unwrap();
    let osc_res = osc_handler.join();
    println!("OSC shutdown: {:?}", osc_res);

    Ok(())
}
