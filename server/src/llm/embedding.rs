pub(super) fn stable_embedding(text: &str, dimensions: usize) -> Vec<f32> {
    let mut embedding = vec![0.0; dimensions];

    for (index, byte) in text.bytes().enumerate() {
        let slot = index % dimensions;
        embedding[slot] += f32::from(byte) / 255.0;
    }

    let norm = embedding
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();

    if norm > 0.0 {
        for value in &mut embedding {
            *value /= norm;
        }
    }

    embedding
}
