use serde_json::{self, Map};
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use serde_bencode;
use clap::{Parser, Subcommand};
use anyhow::Context;
use sha1::{Sha1, Digest};

pub use hashes::Hashes;

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
    }
}


#[derive(Debug, Clone, Deserialize, Serialize)]
struct Torrent {
    announce: String,
    info: Info
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Info {
    name: String,

    #[serde(rename = "piece length")]
    piece_length: usize,

    pieces: Hashes,

    #[serde(flatten)]
    keys: Keys,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum Keys {
    SingleFile {
        length: usize
    },
    MultipleFile {
        files: File
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct File {
    length: usize,
    path: Vec<String>
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

fn main() -> anyhow::Result<()>{
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
        }
    }
    Ok(())
}

mod hashes {
    use serde::de::{self, Deserialize, Deserializer, Visitor};
    use serde::ser::{Serialize, Serializer};
    use std::fmt;

    #[derive(Debug, Clone)]
    pub struct Hashes(Vec<[u8; 20]>);

    struct HashesVisitor;

    impl<'de> Visitor<'de> for HashesVisitor {
        type Value = Hashes;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a byte string whose length is multiple of 20")
        }

        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: de::Error, {
            if v.len() % 20 != 0 {
                return Err(E::custom(format!("length is {}", v.len())));
            } 
            Ok(
                Hashes(
                    v.chunks_exact(20)
                        .map(|slice_20| (slice_20.try_into().expect("length shouls be 20")))
                        .collect()
                )
            )
        }
    }

    impl<'de> Deserialize<'de> for Hashes {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_bytes(HashesVisitor)
        }
    }

    impl Serialize for Hashes {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let single_slice = self.0.concat();
            serializer.serialize_bytes(&single_slice)
        }
    }
}
