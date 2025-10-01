use bittorrent_starter_rust::torrent::{Torrent, Keys};
use bittorrent_starter_rust::tracker::{TrackerRequest, TrackerResponse};
use bittorrent_starter_rust::{peer::*, BLOCK_MAX};
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use std::net::SocketAddrV4;
use std::path::PathBuf;
use serde_bencode;
use clap::{Parser, Subcommand};
use anyhow::Context;
use sha1::{Sha1, Digest};
use serde_json::{self, Map};
use futures_util::{StreamExt, SinkExt};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
#[clap(rename_all = "snake_case")]
pub enum Commands  {
    Decode {
        value: String
    },

    Info {
        torrent: PathBuf
    },

    Peers {
        torrent: PathBuf
    },

    Handshake {
        torrent: PathBuf,
        ip_port: SocketAddrV4
    },

    DownloadPiece {
        #[arg(short)]
        output: PathBuf,
        torrent: PathBuf,
        piece_index: usize
    },
    Download {
        #[arg(short)]
        output: PathBuf,
        torrent: PathBuf,
    },
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
            let info_hash  = t.info_hash();
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
        Commands::Handshake { torrent, ip_port } => {
            let f = std::fs::read(torrent).context("open torrent file")?;
            let t: Torrent = serde_bencode::from_bytes(&f).context("parse torrent file")?;
            let encoded_info = serde_bencode::to_bytes(&t.info).context("reencode info")?;
            let mut hasher = Sha1::new();
            hasher.update(&encoded_info);
            let info_hash  = hasher.finalize();
            let mut peer = tokio::net::TcpStream::connect(ip_port).await.context("connect to peer")?;
            let mut handshake = Handshake::new(info_hash.into(), *b"00112233445566778899");
            {
                let handshake_bytes = handshake.as_bytes_mut();
                peer.write_all(handshake_bytes).await.context("write handshake")?;
                peer.read_exact(handshake_bytes).await.context("read handshake")?;
            }
            assert_eq!(handshake.length, 19);
            assert_eq!(&handshake.bittorrent, b"BitTorrent protocol");
            println!("Peer ID: {}", hex::encode(handshake.peer_id));
        }
        Commands::DownloadPiece { output, torrent, piece_index } => {
            let f = std::fs::read(torrent).context("open torrent file")?;
            let t: Torrent = serde_bencode::from_bytes(&f).context("parse torrent file")?;
            let length = if let Keys::SingleFile { length } = t.info.keys {
                length
            } else {
                todo!()
            };

            assert!(piece_index < t.info.pieces.0.len());

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
            let peer = &response.peers.0[0];
            let mut peer = tokio::net::TcpStream::connect(peer).await.context("connect to peer")?;
            let mut handshake = Handshake::new(info_hash.into(), *b"00112233445566778899");
            {
                let handshake_bytes = handshake.as_bytes_mut();
                peer.write_all(handshake_bytes)
                    .await
                    .context("write handshake")?;
                peer.read_exact(handshake_bytes)
                    .await.context("read handshake")?;
            }
            assert_eq!(handshake.length, 19);
            assert_eq!(&handshake.bittorrent, b"BitTorrent protocol");

            let mut peer = tokio_util::codec::Framed::new(peer, MessageFramer);
            let bitfield = peer
                .next()
                .await
                .expect("peer alway sends bitfield")
                .context("peer message is invalid")?;
            assert_eq!(bitfield.tag, MessageTag::Bitfield);

            peer.send(Message {
                tag: MessageTag::Interested,
                payload: Vec::new(),
            })
                .await
                .context("send interested message")?;

            let unchoke = peer
                .next()
                .await
                .expect("peer alway sends unchoke")
                .context("peer message is invalid")?;
            assert_eq!(unchoke.tag, MessageTag::Unchoke);
            assert!(unchoke.payload.is_empty());

            let piece_hash = &t.info.pieces.0[piece_index];
            let piece_size = if piece_index == t.info.pieces.0.len() - 1 {
                let md = length % t.info.piece_length;
                if md == 0 {
                    t.info.piece_length
                } else {
                    md
                }
            } else {
                t.info.piece_length   
            };

            let nblocks = (piece_size + (BLOCK_MAX - 1)) / BLOCK_MAX;
            let mut all_blocks = Vec::with_capacity(piece_size);

            for block in 0..nblocks {
                let block_size = if block == nblocks - 1 {
                    let md = piece_size % BLOCK_MAX;
                    if md == 0 {
                        BLOCK_MAX
                    } else {
                        md
                    }
                } else {
                    BLOCK_MAX
                };
                let mut request = Request::new(
                    piece_index as u32,
                    (block * BLOCK_MAX) as u32,
                    block_size as u32,
                );
                let request_bytes = Vec::from(request.as_bytes_mut());
                peer.send(Message {
                    tag: MessageTag::Request,
                    payload: request_bytes,
                })
                .await
                .with_context(|| format!("send request for block {block}"))?;

                let piece = peer
                    .next()
                    .await
                    .expect("peer always sends a piece")
                    .context("peer message was invalid")?;
                assert_eq!(piece.tag, MessageTag::Piece);
                assert!(!piece.payload.is_empty());

                let piece = Piece::ref_from_bytes(&piece.payload[..])
                    .expect("always get all Piece response fields from peer");
                assert_eq!(piece.index() as usize, piece_index);
                assert_eq!(piece.begin() as usize, block * BLOCK_MAX);
                assert_eq!(piece.block().len(), block_size);
                all_blocks.extend(piece.block());
            }
            assert_eq!(all_blocks.len(), piece_size);

            let mut hasher = Sha1::new();
            hasher.update(&all_blocks);
            let hash: [u8; 20] = hasher
                .finalize()
                .try_into()
                .expect("GenericArray<_, 20> == [_; 20]");
            assert_eq!(&hash, piece_hash);

            tokio::fs::write(&output, all_blocks)
                .await
                .context("write out downloaded piece")?;
            println!("Piece {piece_index} downloaded to {}.", output.display());
        }
        Commands::Download { output, torrent } => {
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
            let peer = &response.peers.0[0];
            let mut peer = tokio::net::TcpStream::connect(peer).await.context("connect to peer")?;
            let mut handshake = Handshake::new(info_hash.into(), *b"00112233445566778899");
            {
                let handshake_bytes = handshake.as_bytes_mut();
                peer.write_all(handshake_bytes)
                    .await
                    .context("write handshake")?;
                peer.read_exact(handshake_bytes)
                    .await.context("read handshake")?;
            }
            assert_eq!(handshake.length, 19);
            assert_eq!(&handshake.bittorrent, b"BitTorrent protocol");

            let mut peer = tokio_util::codec::Framed::new(peer, MessageFramer);
            let bitfield = peer
                .next()
                .await
                .expect("peer alway sends bitfield")
                .context("peer message is invalid")?;
            assert_eq!(bitfield.tag, MessageTag::Bitfield);

            peer.send(Message {
                tag: MessageTag::Interested,
                payload: Vec::new(),
            })
                .await
                .context("send interested message")?;

            let unchoke = peer
                .next()
                .await
                .expect("peer alway sends unchoke")
                .context("peer message is invalid")?;
            assert_eq!(unchoke.tag, MessageTag::Unchoke);
            assert!(unchoke.payload.is_empty());

            let piece_hashes = &t.info.pieces.0;
            for (piece_index, piece_hash) in piece_hashes.iter().enumerate() {
                let piece_size = if piece_index == piece_hashes.len() - 1 {
                    let md = length % t.info.piece_length;
                    if md == 0 {
                        t.info.piece_length
                    } else {
                        md
                    }
                } else {
                    t.info.piece_length   
                };

                let nblocks = (piece_size + (BLOCK_MAX - 1)) / BLOCK_MAX;
                let mut all_blocks = Vec::with_capacity(piece_size);

                for block in 0..nblocks {
                    let block_size = if block == nblocks - 1 {
                        let md = piece_size % BLOCK_MAX;
                        if md == 0 {
                            BLOCK_MAX
                        } else {
                            md
                        }
                    } else {
                        BLOCK_MAX
                    };
                    let mut request = Request::new(
                        piece_index as u32,
                        (block * BLOCK_MAX) as u32,
                        block_size as u32,
                    );
                    let request_bytes = Vec::from(request.as_bytes_mut());
                    peer.send(Message {
                        tag: MessageTag::Request,
                        payload: request_bytes,
                    })
                        .await
                        .with_context(|| format!("send request for block {block}"))?;

                    let piece = peer
                        .next()
                        .await
                        .expect("peer always sends a piece")
                        .context("peer message was invalid")?;
                    assert_eq!(piece.tag, MessageTag::Piece);
                    assert!(!piece.payload.is_empty());

                    let piece = Piece::ref_from_bytes(&piece.payload[..])
                        .expect("always get all Piece response fields from peer");
                    assert_eq!(piece.index() as usize, piece_index);
                    assert_eq!(piece.begin() as usize, block * BLOCK_MAX);
                    assert_eq!(piece.block().len(), block_size);
                    all_blocks.extend(piece.block());
                }
                assert_eq!(all_blocks.len(), piece_size);

                let mut hasher = Sha1::new();
                hasher.update(&all_blocks);
                let hash: [u8; 20] = hasher
                    .finalize()
                    .try_into()
                    .expect("GenericArray<_, 20> == [_; 20]");
                assert_eq!(&hash, piece_hash);
                
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&output)
                    .await
                    .context("open output file")?;

                tokio::io::AsyncWriteExt::write_all(&mut file, &all_blocks)
                    .await
                    .context("write out downloaded piece")?;
            }
            println!("Downloaded {} to {}.", t.info.name, output.display());
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
