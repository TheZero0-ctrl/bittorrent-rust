use serde::{Serialize, Deserialize};
pub use peers::Peers;

#[derive(Debug, Clone, Serialize)]
pub struct TrackerRequest {
    // pub info_hash: [u8; 20],
    pub peer_id: String,
    pub port: u16,
    pub uploaded: usize,
    pub downloaded: usize,
    pub left: usize,
    pub compact: u8
}

impl TrackerRequest {
    pub fn new(left: usize) -> Self {
        Self {
            peer_id: "00112233445566778899".to_string(),
            port: 6881,
            uploaded: 0,
            downloaded: 0,
            left,
            compact: 1
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrackerResponse {
    pub interval: usize,
    pub peers: Peers
}

mod peers {
    use serde::de::{self, Deserialize, Deserializer, Visitor};
    use serde::ser::{Serialize, Serializer};
    use std::fmt;
    use std::net::{SocketAddrV4, Ipv4Addr};

    #[derive(Debug, Clone)]
    pub struct Peers(pub Vec<SocketAddrV4>);

    struct PeersVisitor;
    impl<'de> Visitor<'de> for PeersVisitor {
        type Value = Peers;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str(
                "6 bytes, The first 4 bytes are the peer's IP address and the last 2 bytes are the peer's port number.",
            )
        }

        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: de::Error, {
            if v.len() % 6 != 0 {
                return Err(E::custom(format!("length is {}", v.len())));
            } 
            Ok(
                Peers(
                    v.chunks_exact(6)
                        .map(|slice_6| {
                            SocketAddrV4::new(
                                Ipv4Addr::new(slice_6[0], slice_6[1], slice_6[2], slice_6[3]),
                                u16::from_be_bytes([slice_6[4], slice_6[5]]),
                            )
                        })
                        .collect()
                )
            )
        }
    }

    impl<'de> Deserialize<'de> for Peers {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_bytes(PeersVisitor)
        }
    }

    impl Serialize for Peers {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut single_slice = Vec::with_capacity(self.0.len() * 6);
            for peer in &self.0 {
                single_slice.extend_from_slice(&peer.ip().octets());
                single_slice.extend_from_slice(&peer.port().to_be_bytes());
            }
            serializer.serialize_bytes(&single_slice)
        }
    }
}
