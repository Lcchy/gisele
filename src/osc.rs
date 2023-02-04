use anyhow::bail;
use num_traits::FromPrimitive;
use rosc::OscMessage;
use std::{io::ErrorKind, net::UdpSocket, sync::Arc};

use crate::{
    midi::midi_pitch_to_note,
    seq::{
        BaseSeqParams::{self},
        EuclidBase, RandomBase, SeqStatus,
    },
    Sequencer,
};

/// Should be enough,See https://osc-dev.create.ucsb.narkive.com/TyotlluU/osc-udp-packet-sizes-for-interoperability
/// and https://www.music.mcgill.ca/~gary/306/week9/osc.html
// const OSC_BUFFER_LEN: usize = 4096;
const OSC_BUFFER_LEN: usize = rosc::decoder::MTU;
pub const OSC_PORT: &str = "34254";

fn osc_handling(osc_msg: &OscMessage, seq: &Arc<Sequencer>) -> anyhow::Result<()> {
    match osc_msg.addr.as_str() {
        "/gisele/set_status" => {
            let status = parse_to_int(osc_msg, 0)?;
            let mut seq_params_mut = seq.params.write();
            seq_params_mut.status = FromPrimitive::from_u32(status as u32)
                .ok_or_else(|| anyhow::format_err!("OSC status arg was not in enum."))?;
            println!("Sequencer Status set to {:?}", seq_params_mut.status);
        }
        "/gisele/set_bpm" => {
            seq.params.write().bpm = parse_to_int(osc_msg, 0)? as u16;
        }
        "/gisele/set_loop_length" => {
            seq.params.write().loop_length = parse_to_int(osc_msg, 0)? as u32;
        }
        "/gisele/regenerate" => {
            let base_seq_id = parse_to_int(osc_msg, 0)? as u32;
            seq.regen_base_seq(base_seq_id)?;
        }
        "/gisele/set_root" => {
            let base_seq_id = parse_to_int(osc_msg, 0)? as u32;
            let target_note = midi_pitch_to_note(parse_to_int(osc_msg, 1)? as u8);
            seq.transpose(base_seq_id, target_note)?;
        }
        "/gisele/set_note_len" => {
            let base_seq_id = parse_to_int(osc_msg, 0)? as u32;
            let note_len = parse_to_int(osc_msg, 1)? as u32;
            seq.change_note_len(base_seq_id, note_len)?;
        }
        "/gisele/empty" => {
            seq.empty();
            seq.params.write().status = SeqStatus::Stop;
        }
        "/gisele/add_random_base" => {
            let root_note = parse_to_int(osc_msg, 0)? as u8;
            let note_len = parse_to_int(osc_msg, 1)? as u32;
            let nb_events = parse_to_int(osc_msg, 2)? as u32;
            let base_seq_params = BaseSeqParams::Random(RandomBase { nb_events });
            seq.add_base_seq(base_seq_params, midi_pitch_to_note(root_note), note_len);
        }
        "/gisele/add_euclid_base" => {
            let root_note = parse_to_int(osc_msg, 0)? as u8;
            let note_len = parse_to_int(osc_msg, 1)? as u32;
            let pulses = parse_to_int(osc_msg, 2)? as u32;
            let steps = parse_to_int(osc_msg, 3)? as u32;
            let base_seq_params = BaseSeqParams::Euclid(EuclidBase { pulses, steps });
            seq.add_base_seq(base_seq_params, midi_pitch_to_note(root_note), note_len);
        }
        "/gisele/random_base/set_nb_events" => {
            let base_seq_id = parse_to_int(osc_msg, 0)? as u32;
            let nb_events = parse_to_int(osc_msg, 1)? as u32;
            seq.set_nb_events(base_seq_id, nb_events)?;
        }
        _ => bail!("OSC path was not recognized"),
    }
    println!("Osc command success.");
    Ok(())
}

/// Returns the main osc receiving loop
pub fn osc_process_closure(
    udp_socket: UdpSocket,
    seq: Arc<Sequencer>,
) -> impl FnOnce() -> anyhow::Result<()> {
    move || {
        let mut rec_buffer = [0; OSC_BUFFER_LEN];
        while seq.params.read().status != SeqStatus::Shutdown {
            match udp_socket.recv(&mut rec_buffer) {
                Ok(received) => {
                    let (_, packet) =
                        if let Ok(v) = rosc::decoder::decode_udp(&rec_buffer[..received]) {
                            v
                        } else {
                            println!("OSC message could not be decoded.");
                            continue;
                        };
                    match packet {
                        rosc::OscPacket::Message(msg) => {
                            println!("Received osc msg {:?}", msg);
                            let r = osc_handling(&msg, &seq);
                            if let Err(e) = r {
                                println!("OSC message handling failed with: {:?}", e);
                            }
                        }
                        rosc::OscPacket::Bundle(_) => unimplemented!(),
                    }
                }
                Err(e) => {
                    // Letting timeout errs pass silently
                    if e.kind() != ErrorKind::WouldBlock {
                        eprintln!("recv function failed: {:?}", e);
                    }
                }
            }
        }
        println!("Osc process shutdown gracefully.");
        Ok(())
    }
}

fn parse_to_int(osc_msg: &OscMessage, arg_idx: usize) -> anyhow::Result<i32> {
    osc_msg.args[arg_idx]
        .to_owned()
        .int()
        .ok_or_else(|| anyhow::format_err!("OSC arg nb {} was not recognized.", arg_idx))
}
