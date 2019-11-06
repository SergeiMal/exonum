// Copyright 2019 The Exonum Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Module of the rust-protobuf generated files.

// For protobuf generated files.
#![allow(bare_trait_objects)]

use failure::Error;
use protobuf::{well_known_types::Empty, RepeatedField};
use std::borrow::Cow;

use crate::{proof_map_index::ProofPath, BinaryKey, BinaryValue};
use exonum_crypto::proto::*;
use exonum_proto::ProtobufConvert;

pub use self::{list_proof::*, map_proof::*};

include!(concat!(env!("OUT_DIR"), "/protobuf_mod.rs"));

impl<K, V> ProtobufConvert for crate::MapProof<K, V>
where
    K: BinaryKey + ToOwned<Owned = K>,
    V: BinaryValue,
{
    type ProtoStruct = MapProof;

    fn to_pb(&self) -> Self::ProtoStruct {
        let mut map_proof = MapProof::new();

        let proof: Vec<MapProofEntry> = self
            .proof_unchecked()
            .iter()
            .map(|(p, h)| {
                let mut entry = MapProofEntry::new();
                entry.set_hash(h.to_pb());
                entry.set_proof_path(p.as_bytes().to_vec());
                entry
            })
            .collect();

        let entries: Vec<OptionalEntry> = self
            .all_entries_unchecked()
            .map(|(key, value)| {
                let mut entry = OptionalEntry::new();
                let mut buf = vec![0_u8; key.size()];
                key.write(&mut buf);
                entry.set_key(buf.to_vec());

                match value {
                    Some(value) => entry.set_value(value.to_bytes()),
                    None => entry.set_no_value(Empty::new()),
                }

                entry
            })
            .collect();

        map_proof.set_proof(RepeatedField::from_vec(proof));
        map_proof.set_entries(RepeatedField::from_vec(entries));

        map_proof
    }

    fn from_pb(pb: Self::ProtoStruct) -> Result<Self, Error> {
        let proof = pb
            .get_proof()
            .iter()
            .map(|entry| {
                Ok((
                    ProofPath::read(entry.get_proof_path()),
                    exonum_crypto::Hash::from_pb(entry.get_hash().clone())?,
                ))
            })
            .collect::<Result<Vec<_>, Error>>()?;

        let entries = pb
            .get_entries()
            .iter()
            .map(|entry| {
                let key = K::read(entry.get_key());

                let value = if entry.has_value() {
                    Some(V::from_bytes(Cow::Borrowed(entry.get_value()))?)
                } else {
                    ensure!(
                        entry.has_no_value(),
                        "malformed message, no_value is absent"
                    );
                    None
                };

                Ok((key, value))
            })
            .collect::<Result<Vec<_>, Error>>()?;

        let mut map_proof = crate::MapProof::new().add_proof_entries(proof);

        for entry in entries {
            map_proof = match entry.1 {
                Some(value) => map_proof.add_entry(entry.0, value),
                None => map_proof.add_missing(entry.0),
            };
        }

        Ok(map_proof)
    }
}

#[cfg(test)]
mod tests {
    use exonum_crypto::{proto::types, PublicKey};
    use exonum_proto::ProtobufConvert;
    use std::fmt;

    use crate::{
        proto, BinaryKey, BinaryValue, Database, ListProof, MapProof, ObjectHash, ProofListIndex,
        ProofMapIndex, TemporaryDB,
    };
    use protobuf::RepeatedField;

    #[test]
    fn serialize_map_proof() {
        let db = TemporaryDB::default();
        let storage = db.fork();

        let mut table = ProofMapIndex::new("index", &storage);

        let proof = table.get_proof(0);
        assert_proof_roundtrip(&proof);

        for i in 0..10 {
            table.put(&i, i);
        }

        let proof = table.get_proof(5);
        assert_proof_roundtrip(&proof);

        let proof = table.get_multiproof(5..15);
        assert_proof_roundtrip(&proof);
    }

    fn assert_proof_roundtrip<K, V>(proof: &MapProof<K, V>)
    where
        K: BinaryKey + ObjectHash + fmt::Debug,
        V: BinaryValue + ObjectHash + fmt::Debug,
        MapProof<K, V>: ProtobufConvert + PartialEq,
    {
        let pb = proof.to_pb();
        let deserialized: MapProof<K, V> = MapProof::from_pb(pb).unwrap();
        let checked_proof = deserialized
            .check()
            .expect("deserialized proof is not valid");

        assert_eq!(proof, &deserialized);
        assert_eq!(
            checked_proof.index_hash(),
            proof.check().unwrap().index_hash()
        );
    }

    #[test]
    fn map_proof_malformed_serialize() {
        let mut proof = proto::MapProof::new();
        let mut proof_entry = proto::MapProofEntry::new();
        let mut hash = types::Hash::new();

        hash.set_data(vec![0_u8; 31]);
        proof_entry.set_hash(hash);
        proof_entry.set_proof_path(vec![0_u8; 34]);
        proof.set_proof(RepeatedField::from_vec(vec![proof_entry]));

        let res = MapProof::<u8, u8>::from_pb(proof.clone());
        assert!(res.unwrap_err().to_string().contains("Wrong Hash size"));

        let mut entry = proto::OptionalEntry::new();
        entry.set_key(vec![0_u8; 32]);
        proof.clear_proof();
        proof.set_entries(RepeatedField::from_vec(vec![entry]));

        let res = MapProof::<PublicKey, u8>::from_pb(proof.clone());
        assert!(res.unwrap_err().to_string().contains("malformed message"));
    }

    #[test]
    #[should_panic]
    fn map_proof_malformed_key_serialize() {
        let mut proof = proto::MapProof::new();
        let mut proof_entry = proto::MapProofEntry::new();
        proof_entry.set_proof_path(vec![0_u8; 33]);
        proof.set_proof(RepeatedField::from_vec(vec![proof_entry]));

        // TODO: will panic at runtime, should change BinaryKey::read signature (ECR-174)
        let _res = MapProof::<u8, u8>::from_pb(proof);
    }

    #[test]
    fn serialize_list_proof() {
        let db = TemporaryDB::default();
        let storage = db.fork();

        let mut table = ProofListIndex::new("index", &storage);

        let proof = table.get_proof(0);
        assert_list_proof_roundtrip(&proof);

        for i in 0..256 {
            table.push(i);
        }

        let proof = table.get_proof(5);
        assert_list_proof_roundtrip(&proof);

        let proof = table.get_range_proof(250..260);
        assert_list_proof_roundtrip(&proof);
    }

    fn assert_list_proof_roundtrip<V>(proof: &ListProof<V>)
    where
        V: BinaryValue + ObjectHash + std::fmt::Debug,
        ListProof<V>: ProtobufConvert + PartialEq,
    {
        let pb = proof.to_pb();
        let deserialized: ListProof<V> = ListProof::from_pb(pb).unwrap();
        let checked_proof = deserialized
            .check()
            .expect("deserialized proof is not valid");

        assert_eq!(proof, &deserialized);
        assert_eq!(
            checked_proof.index_hash(),
            proof.check().unwrap().index_hash()
        );
    }
}