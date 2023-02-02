use anyhow::Result;
use jack::{Client, ClientOptions, RawMidi};
use osc::{osc_process_closure, OSC_PORT};
use seq::{EventType, SeqInternalStatus, SeqStatus};
use std::{io, net::UdpSocket, sync::Arc, thread, time::Duration};

use crate::seq::Sequencer;

mod midi;
mod osc;
mod seq;

const INIT_BPM: u16 = 120;

fn main() -> Result<()> {
    // Set up jack ports
    let (jclient, _) = Client::new("gisele_jack", ClientOptions::NO_START_SERVER)?;

    let mut out_port = jclient
        .register_port("gisele_out", jack::MidiOut::default())
        .unwrap();

    // Init values
    let seq_arc = Arc::new(Sequencer::new(INIT_BPM, 16));
    let seq_ref = seq_arc.clone();

    // Define the Jack process
    let jack_process = move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        let seq_params = seq_ref.params.read();
        let mut seq_int = seq_ref.internal.write();

        // Handle Sequencer statuses
        if seq_params.status == SeqStatus::Start {
            seq_int.status = SeqInternalStatus::Playing;
        }
        if seq_int.status == SeqInternalStatus::Silence {
            return jack::Control::Continue;
        }

        let event_buffer = &*seq_ref.event_buffer.read();
        let mut out_buff = out_port.writer(ps);
        let loop_len = seq_params.get_loop_len_in_us();
        let cy_times = ps.cycle_times().unwrap();

        // we increment the current jack process cycle time window dynamically to allow speed playback variations
        println!("seq_int.j_window_time_end {}", seq_int.j_window_time_end);
        println!("cy_times.next_usecs {}", cy_times.next_usecs);
        println!("cy_times.current_usecs {}", cy_times.current_usecs);
        println!("seq_params.bpm {}", seq_params.bpm);
        println!("loop_len {}", loop_len);
        // We loop the start in case of a loop_len variation due to bpm change
        seq_int.j_window_time_start %= loop_len;
        seq_int.j_window_time_end = (seq_int.j_window_time_end
            + (((cy_times.next_usecs - cy_times.current_usecs) as f64)
                * (seq_params.bpm as f64 / INIT_BPM as f64)) as u64)
            % loop_len;

        let new_curr_bar = (seq_int.j_window_time_end / seq_params.get_step_len_in_us()) as u32;
        let new_curr_bar_s = (seq_int.j_window_time_start / seq_params.get_step_len_in_us()) as u32;
        println!("Current bar start: {}", new_curr_bar_s);
        println!("Current bar end: {}", new_curr_bar);
        if new_curr_bar != seq_int.curr_bar {
            seq_int.curr_bar = new_curr_bar;
        }

        let event_head_before = *seq_ref.event_head.read();
        let halting = seq_params.status == SeqStatus::Pause || seq_params.status == SeqStatus::Stop;
        loop {
            let curr_event_head = *seq_ref.event_head.read();
            if let Some(next_event) = &event_buffer.get(curr_event_head) {
                // println!("Next note Time {}", next_event.time);

                let next_event_time = (next_event.bar_pos as u64) * seq_params.get_step_len_in_us();
                let mut push_event = seq_int.event_in_cycle(next_event_time);

                // We let the seq play once through all midi off notes when halting.
                let mut jump_event = false;
                if halting {
                    if let EventType::MidiNote(n) = next_event.e_type {
                        if !n.on_off {
                            // Let through all midi off msgs for Pause and stop.
                            push_event = true;
                        } else if (*seq_ref.event_head.read() + 1) % event_buffer.len()
                            != event_head_before
                        {
                            jump_event = true;
                        }
                    }
                };
                jump_event = jump_event || loop_len < next_event_time;

                if jump_event {
                    seq_ref.incr_event_head();
                } else if push_event {
                    match next_event.e_type {
                        EventType::MidiNote(ref note) => {
                            let raw_midi = RawMidi {
                                time: ps.frames_since_cycle_start(),
                                bytes: &note.get_raw_note_on_bytes(),
                            };
                            // Max event buff size was measured at ~32kbits ? In practice, 800-2200 midi msgs
                            out_buff.write(&raw_midi).unwrap();
                            println!(
                            "Sending midi note: Channel {:<5} Pitch {:<5} Vel {:<5} On/Off {:<5} Note pos in bars {}",
                            note.channel, note.pitch, note.velocity, note.on_off, next_event.bar_pos
                        );
                        }
                        EventType::_Fill => todo!(),
                    }
                    seq_ref.incr_event_head();
                } else {
                    // Complete the current cycle when reaching a note to be played in the next one
                    break;
                }

                // Stop when having played all noteOffs in loop before pause/stop
                if *seq_ref.event_head.read() == event_head_before {
                    break;
                }
            } else {
                break;
            }
        }

        seq_int.j_window_time_start = seq_int.j_window_time_end;

        // Reset the seq to start or current position in case of a stop or pause
        if seq_params.status == SeqStatus::Pause || seq_params.status == SeqStatus::Stop {
            *seq_ref.event_head.write() = event_head_before;
            seq_int.status = SeqInternalStatus::Silence;
        }
        if seq_params.status == SeqStatus::Stop {
            println!("Sequencer Stopped.");
            seq_ref.stop_reset(seq_int);
        }

        jack::Control::Continue
    };

    // Start the Jack thread
    let process = jack::ClosureProcessHandler::new(jack_process);
    let active_client = jclient.activate_async((), process).unwrap();

    // Start the OSC listening thread
    let udp_socket = UdpSocket::bind(format!("127.0.0.1:{}", OSC_PORT))?;
    // Setting the UDP recv timeout to 1s to allow for gracefull shutdown
    udp_socket.set_read_timeout(Some(Duration::from_secs(1)))?;
    let osc_process = osc_process_closure(udp_socket, seq_arc.clone());
    let osc_handler = thread::spawn(osc_process);

    // Graceful shutdown on user input
    println!("Press enter/return to quit...");
    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input).ok();
    seq_arc.params.write().status = SeqStatus::Shutdown;
    active_client.deactivate().unwrap();
    println!("Jack process shutdown.");
    println!("Waiting for OSC process...");
    osc_handler.join().unwrap()?;

    Ok(())
}
