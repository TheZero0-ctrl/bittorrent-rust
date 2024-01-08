use bittorrent_starter_rust::torrent::{Torrent, Keys};
use bittorrent_starter_rust::tracker::{TrackerRequest, TrackerResponse};
use std::path::PathBuf;
use serde_bencode;
use clap::{Parser, Subcommand};
use anyhow::Context;
use sha1::{Sha1, Digest};
use serde_json::{self, Map};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands  {
    Decode {
        value: String
    },

    Info {
        torrent: PathBuf
    },

    Peers {
        torrent: PathBuf
    }
}

fn decode_bencoded_value(encoded_value: &str) -> (serde_json::Value, &str) {
    match encoded_value.chars().next() {
       Some('i') => {
            if let Some((n, rest)) = encoded_value
                .split_at(1)
                .1
                .split_once('e')
                .and_then(|(digit, rest)| {
                    let n = digit.parse::<i64>().ok()?;
                    Some((n, rest))
                })
            {
                return (n.into(), rest);
            }
            else {
                panic!("Unhandled encoded value: {}", encoded_value)
            }
        }
        Some('l') => {
            let mut values = Vec::new();
            let mut rest = encoded_value.split_at(1).1;
            while !rest.is_empty() && !rest.starts_with('e') {
                let (v, remainder) = decode_bencoded_value(rest);   
                values.push(v);
                rest = remainder;
            }
            return (values.into(), &rest[1..]);
        }
        Some('d') => {
            let mut dicts = Map::new();
            let mut rest = encoded_value.split_at(1).1;
            while !rest.is_empty() && !rest.starts_with('e') {
                let (k, remainder) = decode_bencoded_value(rest);
                let k = match k {
                   serde_json::Value::String(k) => k,
                    k => {
                        panic!("dict key must be string not {k:?}");
                    }
                };
                let (v, remainder) = decode_bencoded_value(remainder);
                dicts.insert(k, v);
                rest = remainder;
            }
            return (dicts.into(), &rest[1..]);
        }
        Some('0'..='9') => {
            if let Some((len, rest)) = encoded_value.split_once(':') {
                if let Ok(len) = len.parse::<usize>() {
                    return (rest[..len].to_string().into(), &rest[len..]);
                }
                else {
                    panic!("Unhandled encoded value: {}", encoded_value)
                }
            }
            else {
                panic!("Unhandled encoded value: {}", encoded_value)
            }
        }
        _ => {
            panic!("Unhandled encoded value: {}", encoded_value)
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()>{
    let args = Args::parse();
    match args.command {
        Commands::Decode { value }  => {
            let decoded_value = decode_bencoded_value(&value);
            println!("{}", decoded_value.0.to_string());
        } 
        Commands::Info { torrent } => {
            let f = std::fs::read(torrent).context("open torrent file")?;
            let t: Torrent = serde_bencode::from_bytes(&f).context("parse torrent file")?;
            println!("Tracker URL: {}", t.announce);
            if let Keys::SingleFile { length } = t.info.keys {
                println!("Length: {}", length);
            }
            let encoded_info = serde_bencode::to_bytes(&t.info).context("reencode info")?;
            let mut hasher = Sha1::new();
            hasher.update(&encoded_info);
            let info_hash  = hasher.finalize();
            println!("Info Hash: {}", hex::encode(&info_hash));
            println!("Piece Length: {}", t.info.piece_length);
            println!("Piece Hashes:");
            for hash in t.info.pieces.0 {
                println!("{}", hex::encode(hash))
            }
        }
        Commands::Peers { torrent } => {
            let f = std::fs::read(torrent).context("open torrent file")?;
            let t: Torrent = serde_bencode::from_bytes(&f).context("parse torrent file")?;
            let length = if let Keys::SingleFile { length } = t.info.keys {
                length
            } else {
                todo!()
            };
            let encoded_info = serde_bencode::to_bytes(&t.info).context("reencode info")?;
            let mut hasher = Sha1::new();
            hasher.update(&encoded_info);
            let info_hash  = hasher.finalize();
            let request = TrackerRequest::new(length);
            let url_params = serde_urlencoded::to_string(&request).unwrap();
            let tracker_url = format!(
                "{}?{}&info_hash={}",
                t.announce,
                url_params,
                urlencode(&info_hash.into()),
            );
            let response = reqwest::get(tracker_url).await?.bytes().await?;
            let response: TrackerResponse = serde_bencode::from_bytes(&response).unwrap();
            for peer in response.peers.0 {
                println!("{}:{}", peer.ip(), peer.port());
            }
        }
    }
    Ok(())
}
fn urlencode(t: &[u8; 20]) -> String {
    let mut encoded = String::with_capacity(3 * t.len());
    for &byte in t {
        encoded.push('%');
        encoded.push_str(&hex::encode(&[byte]));
    }
    encoded
}
