//! Offline reference vocabulary and contextual ranking, shared by desktop hosts.
mod ranking;
mod syntax;
mod vocabulary;
pub use ranking::{name_match_rank, rank_indices, CandidateSource, RankCandidate, UsageWeight};
pub use syntax::{literal_command_name, simple_command_words};
pub use vocabulary::{
    suggestions as reference_suggestions, ReferenceSuggestion, VocabularyContext,
};
