//! CLIP embeddings: a local ONNX model, vectors in the DB, tags, cosine search, k-means.
//!
//! The models live as files in the folder from the settings (`~/.cache/photocat/models/` by
//! default) and are checked by sha256. The network is used only for weights from the catalogue, with the owner's consent (ADR 0015).
//!
//! Split by role (M27.5): `model.rs` is the catalogue and the ONNX session, `tags.rs` the
//! dictionary and the two passes, `search.rs` the cosine search and the tag filters, `kmeans.rs` the clusters and
//! `download.rs` the weights. This module re-exports the public names unchanged.

mod download;
mod kmeans;
mod model;
mod search;
mod tags;

#[cfg(test)]
mod tests;

pub use download::*;
pub use kmeans::*;
pub use model::*;
pub use search::*;
pub use tags::*;
