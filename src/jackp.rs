use crate::seq::{Event, EventType, SeqInternalStatus, SeqStatus};
use jack::{Client, Control, MidiOut, MidiWriter, Port, ProcessScope, RawMidi};
use std::sync::Arc;

use crate::seq::Sequencer;

/// Define the Jack process
pub(crate) fn jack_process_closure(
    seq_ref: Arc<Sequencer>,
    mut midi_out: Port<MidiOut>,
) -> impl FnMut(&Client, &ProcessScope) -> Control {
    move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        let seq_params = seq_ref.params.read();
        let mut seq_int = seq_ref.internal.write();

        // Handle Sequencer statuses
        if seq_params.status == SeqStatus::Start {
            seq_int.status = SeqInternalStatus::Playing;
        }
        if seq_int.status == SeqInternalStatus::Silence {
            return jack::Control::Continue;
        }

        // Increment the current jack process time window dynamically to allow for speed playback variations
        let cy_times = ps.cycle_times().unwrap();
        seq_int.j_window_time_start = seq_int.j_window_time_end;
        seq_int.j_window_time_end += (seq_params.bpm as f64
            * (cy_times.next_usecs as f64 - cy_times.current_usecs as f64))
            / 6e7;

        // Print out current bar
        let new_curr_bar = seq_int.j_window_time_end as u32;
        if new_curr_bar != seq_int.curr_bar {
            seq_int.curr_bar = new_curr_bar;
            println!("Current bar: {new_curr_bar} ({})", new_curr_bar % 16);
        }

        // In case of pause/stop, send notes off and reset sequencer
        let mut out_buff = midi_out.writer(ps);
        if seq_params.status == SeqStatus::Pause || seq_params.status == SeqStatus::Stop {
            seq_ref.notes_off(ps, &mut out_buff);
            if seq_params.status == SeqStatus::Stop {
                // Reset the seq to start or current position in case of a stop or pause
                println!("Sequencer Stopped.");
                seq_ref.reset_base_seqs();
                seq_int.j_window_time_start = 0.;
                seq_int.j_window_time_end = 0.;
            }
            seq_int.status = SeqInternalStatus::Silence;
            return jack::Control::Continue;
        }
        drop(seq_int);

        for base_seq in &*seq_ref.base_seqs.read() {
            let loop_len = base_seq.params.read().loop_length;
            let event_buffer = &base_seq.event_buffer.read();

            loop {
                let curr_event_head = *base_seq.event_head.read();
                if let Some(next_event) = event_buffer.get(curr_event_head) {
                    let push_event = seq_ref
                        .internal
                        .read()
                        .event_in_cycle(next_event.bar_pos as f64, loop_len);

                    if loop_len <= next_event.bar_pos {
                        base_seq.incr_event_head();
                    } else if push_event {
                        let mut process_event = next_event.clone();
                        seq_ref.process_event(&base_seq.fx_proc_ids.read(), &mut process_event);
                        send_event(ps, &mut out_buff, &process_event);
                        base_seq.incr_event_head();
                    } else {
                        // Complete the current cycle when reaching a note to be played in the next one
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        jack::Control::Continue
    }
}

/// Push an event to the jack output buffer
pub(crate) fn send_event(ps: &jack::ProcessScope, out_buff: &mut MidiWriter, next_event: &Event) {
    match next_event.e_type {
        EventType::MidiNote(ref note) => {
            let raw_midi = RawMidi {
                time: ps.frames_since_cycle_start(),
                bytes: &note.get_raw_note_on_bytes(),
            };
            // Max event buff size was measured at ~32kbits ? In practice, 800-2200 midi msgs
            if let Err(e) = out_buff.write(&raw_midi) {
                eprintln!("Could not insert in jack output buffer: {e}");
            };
            println!(
        "Sending midi note: Channel {:<5} Pitch {:<5} Vel {:<5} On/Off {:<5} Note pos in bars {}",
        note.channel, note.pitch, note.velocity, note.on_off, next_event.bar_pos
    );
        }
        EventType::_Fill => todo!(),
    }
}
