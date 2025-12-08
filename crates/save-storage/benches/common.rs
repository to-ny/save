use rand::Rng;

pub fn random_bytes(size: usize) -> Vec<u8> {
    let mut rng = rand::rng();
    (0..size).map(|_| rng.random::<u8>()).collect()
}

pub fn format_size(bytes: usize) -> String {
    if bytes >= 1_048_576 {
        format!("{}MB", bytes / 1_048_576)
    } else if bytes >= 1024 {
        format!("{}KB", bytes / 1024)
    } else {
        format!("{}B", bytes)
    }
}
