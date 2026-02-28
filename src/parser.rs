use crate::state::{PackageUpdate, UpdateSource};

pub fn parse_update_lines(output: &str, source: UpdateSource) -> Vec<PackageUpdate> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| parse_update_line(line, source))
        .collect()
}

fn parse_update_line(line: &str, source: UpdateSource) -> Option<PackageUpdate> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }

    let name = parts.first()?.to_string();
    let current = parts.get(1)?.to_string();

    let latest = if let Some(arrow_idx) = parts.iter().position(|p| *p == "->") {
        parts
            .get(arrow_idx + 1)
            .unwrap_or(parts.last()?)
            .to_string()
    } else {
        parts.last()?.to_string()
    };

    Some(PackageUpdate {
        name,
        current,
        latest,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_official_format() {
        let input = "pacman 6.1.0-1 -> 6.1.1-1\nopenssl 3.1.5-1 -> 3.1.6-1\n";
        let parsed = parse_update_lines(input, UpdateSource::Official);

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "pacman");
        assert_eq!(parsed[0].current, "6.1.0-1");
        assert_eq!(parsed[0].latest, "6.1.1-1");
        assert_eq!(parsed[1].name, "openssl");
    }

    #[test]
    fn parses_aur_format() {
        let input = "google-chrome 125.0.1-1 -> 125.0.2-1\n";
        let parsed = parse_update_lines(input, UpdateSource::Aur);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "google-chrome");
        assert_eq!(parsed[0].latest, "125.0.2-1");
        assert_eq!(parsed[0].source, UpdateSource::Aur);
    }

    #[test]
    fn skips_invalid_lines() {
        let input = "\nwarning line\nfoo 1 -> 2\n";
        let parsed = parse_update_lines(input, UpdateSource::Official);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "foo");
    }
}
