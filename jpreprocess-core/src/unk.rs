use once_cell::sync::Lazy;

use crate::{node_details::NodeDetails, pos::PartOfSpeech, pronounciation::Pronounciation};

pub const UNK: Lazy<NodeDetails> = Lazy::new(|| NodeDetails {
    pos: PartOfSpeech::new(["名詞", "*", "*", "*"]),
    is_renyou: false,
    orig: "*".to_string(),
    read: None,
    pron: Pronounciation::default(),
    acc: 0,
    mora_size: 0,
    chain_rule: None,
    chain_flag: None,
});
