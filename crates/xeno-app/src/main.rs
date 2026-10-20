//! The Xenobots game binary. For now it only reports its version.

/// The name and version shown to the user, e.g. `xeno-app 0.1.0`.
fn version_string() -> String {
    format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

fn main() {
    println!("{}", version_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_string_names_the_binary_and_version() {
        let s = version_string();
        assert!(s.starts_with("xeno-app "));
        assert!(s.ends_with(env!("CARGO_PKG_VERSION")));
    }
}
