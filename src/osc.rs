use std::{net::UdpSocket, sync::Arc};

use crate::seq::BaseSeqParams::{self, Random};
use crate::seq::{EuclidBase, RandomBase};
use crate::{midi::midi_pitch_to_note, seq::BaseSeq, Sequencer};
use anyhow::bail;
use num_traits::FromPrimitive;
use rosc::OscMessage;

/// Should be enough,See https://osc-dev.create.ucsb.narkive.com/TyotlluU/osc-udp-packet-sizes-for-interoperability
/// and https://www.music.mcgill.ca/~gary/306/week9/osc.html
// const OSC_BUFFER_LEN: usize = 4096;
const OSC_BUFFER_LEN: usize = rosc::decoder::MTU;
pub const OSC_PORT: &str = "34254";

fn osc_handling(osc_msg: &OscMessage, seq: &Arc<Sequencer>) -> anyhow::Result<()> {
    match osc_msg.addr.as_str() {
        "/gisele/set_status" => {
            let status = osc_msg.args[0]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC status arg was not recognized."))?;
            let mut seq_params_mut = seq.params.write().unwrap();
            seq_params_mut.status = FromPrimitive::from_u32(status as u32)
                .ok_or_else(|| anyhow::format_err!("OSC status arg was not in enum."))?;
            println!("Sequencer Status set to {:?}", seq_params_mut.status);
        }
        "/gisele/set_bpm" => {
            let mut seq_params_mut = seq.params.write().unwrap();
            seq_params_mut.bpm = osc_msg.args[0]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC bpm arg wass not recognized."))?
                as u16;
        }
        "/gisele/set_loop_length" => {
            let mut seq_params_mut = seq.params.write().unwrap();
            seq_params_mut.loop_length = osc_msg.args[1]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC loop_len arg was not recognized."))?
                as u64;
        }
        "/gisele/regenerate" => {
            let base_seq_id = osc_msg.args[0]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC base_seq_id arg was not recognized."))?
                as u32;
            let seq_params = seq.params.read().unwrap();
            let base_seq = seq_params
                .base_seqs
                .iter()
                .find(|s| s.id == base_seq_id)
                .ok_or_else(|| anyhow::format_err!("Base sequence could not be found."))?;
            println!("Regenerating base sequence..");
            seq.regen_base_seq(base_seq);
            println!("Finished regenerating");
        }
        "/gisele/set_root" => {
            let base_seq_id = osc_msg.args[0]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC base_seq_id arg was not recognized."))?
                as u32;
            let mut seq_params_mut = seq.params.write().unwrap();
            let base_seq_mut = seq_params_mut
                .base_seqs
                .iter_mut()
                .find(|s| s.id == base_seq_id)
                .ok_or_else(|| anyhow::format_err!("Base sequence could not be found."))?;
            let target_note =
                midi_pitch_to_note(
                    osc_msg.args[1].to_owned().int().ok_or_else(|| {
                        anyhow::format_err!("OSC root_note arg was not recognized.")
                    })? as u8,
                );
            seq.transpose(base_seq_mut, target_note);
        }
        "/gisele/set_note_len" => {
            let base_seq_id = osc_msg.args[0]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC base_seq_id arg was not recognized."))?
                as u32;
            let mut seq_params_mut = seq.params.write().unwrap();
            let base_seq_mut = seq_params_mut
                .base_seqs
                .iter_mut()
                .find(|s| s.id == base_seq_id)
                .ok_or_else(|| anyhow::format_err!("Base sequence could not be found."))?;
            base_seq_mut.note_len = osc_msg.args[1]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC root_note arg was not recognized."))?
                as u8;
            println!("Regenerating base sequence..");
            seq.regen_base_seq(base_seq_mut);
            println!("Finished regenerating");
        }
        "/gisele/empty" => {
            seq.empty();
            println!("Finished emptying");
        }
        "/gisele/add_random_base" => {
            let root_note = osc_msg.args[0]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC root_note arg was not recognized."))?
                as u8;
            let note_len = osc_msg.args[1]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC note_len arg was not recognized."))?
                as u8;
            let nb_events = osc_msg.args[2]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC nb_events arg was not recognized."))?
                as u64;
            let base_seq_params = BaseSeqParams::Random(RandomBase { nb_events });
            println!("Inserting..");
            seq.add_base_seq(base_seq_params, midi_pitch_to_note(root_note), note_len);
            println!("Finished inserting");
        }
        "/gisele/add_euclid_base" => {
            let root_note = osc_msg.args[0]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC root_note arg was not recognized."))?
                as u8;
            let note_len = osc_msg.args[1]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC note_len arg was not recognized."))?
                as u8;
            let pulses = osc_msg.args[2]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC pulses arg was not recognized."))?
                as u8;
            let steps = osc_msg.args[3]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC steps arg was not recognized."))?
                as u8;
            let base_seq_params = BaseSeqParams::Euclid(EuclidBase { pulses, steps });
            println!("Inserting..");
            seq.add_base_seq(base_seq_params, midi_pitch_to_note(root_note), note_len);
            println!("Finished inserting");
        }
        "/gisele/random_base/set_nb_events" => {
            let base_seq_id = osc_msg.args[0]
                .to_owned()
                .int()
                .ok_or_else(|| anyhow::format_err!("OSC base_seq_id arg was not recognized."))?
                as u32;
            let mut seq_params_mut = seq.params.write().unwrap();
            let base_seq_mut = seq_params_mut
                .base_seqs
                .iter_mut()
                .find(|s| s.id == base_seq_id)
                .ok_or_else(|| anyhow::format_err!("Base sequence could not be found."))?;
            if let BaseSeq {
                ty: Random(RandomBase { ref mut nb_events }),
                ..
            } = base_seq_mut
            {
                *nb_events =
                    osc_msg.args[1].to_owned().int().ok_or_else(|| {
                        anyhow::format_err!("OSC nb_events arg was not recognized.")
                    })? as u64;
                println!("Reseeding..");
                seq.regen_base_seq(base_seq_mut);
                println!("Finished reseeding");
            } else {
                eprintln!("The given base_seq_id is wrong.")
            };
        }

        _ => bail!("OSC path was not recognized"),
    }
    Ok(())
}

/// Returns the main osc receiving loop
pub fn osc_process_closure(
    udp_socket: UdpSocket,
    params_ref: Arc<Sequencer>,
) -> impl FnOnce() -> anyhow::Result<()> {
    move || {
        let mut rec_buffer = [0; OSC_BUFFER_LEN];
        loop {
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
                            let r = osc_handling(&msg, &params_ref);
                            if let Err(e) = r {
                                println!("OSC message handling failed with: {:?}", e);
                            }
                        }
                        rosc::OscPacket::Bundle(_) => unimplemented!(),
                    }
                }
                Err(e) => println!("recv function failed: {:?}", e),
            }
        }
    }
}
