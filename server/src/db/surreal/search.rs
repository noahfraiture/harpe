const INDEXED_EMBEDDING_DIMENSIONS: &[IndexedEmbeddingDimension] = &[
    IndexedEmbeddingDimension {
        dimensions: 16,
        field: "embedding_16",
    },
    IndexedEmbeddingDimension {
        dimensions: 384,
        field: "embedding_384",
    },
    IndexedEmbeddingDimension {
        dimensions: 768,
        field: "embedding_768",
    },
    IndexedEmbeddingDimension {
        dimensions: 1024,
        field: "embedding_1024",
    },
    IndexedEmbeddingDimension {
        dimensions: 1536,
        field: "embedding_1536",
    },
    IndexedEmbeddingDimension {
        dimensions: 3072,
        field: "embedding_3072",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IndexedEmbeddingDimension {
    dimensions: usize,
    field: &'static str,
}

pub(super) fn lexical_score(query: &str, content: &str) -> f32 {
    if query.is_empty() {
        return 0.0;
    }

    let content = content.to_lowercase();
    let terms = query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();

    if terms.is_empty() {
        return 0.0;
    }

    let matches = terms.iter().filter(|term| content.contains(**term)).count();

    matches as f32 / terms.len() as f32
}

pub(super) fn indexed_embedding_field(dimensions: usize) -> Option<&'static str> {
    INDEXED_EMBEDDING_DIMENSIONS
        .iter()
        .find(|indexed| indexed.dimensions == dimensions)
        .map(|indexed| indexed.field)
}

pub(super) fn fixed_embedding(embedding: &[f32], dimensions: usize) -> Option<Vec<f32>> {
    (embedding.len() == dimensions).then(|| embedding.to_vec())
}
