const PATH_SEPARATOR: char = '/';
const HOST_SEPARATOR: char = '.';
const PORT_SEPARATOR: char = ':';
const SEPARATORS: [&str; 3] = [".", "_", "__"];

pub fn resource(name: &str) -> bool {
    let mut segments = name.split(PATH_SEPARATOR);
    let Some(first) = segments.next() else {
        return false;
    };
    let rest: Vec<&str> = segments.collect();
    let leading = component(first) || (!rest.is_empty() && hostname(first));
    leading && rest.into_iter().all(component)
}

pub fn repository(name: &str) -> bool {
    name.split(PATH_SEPARATOR).all(component)
}

fn component(segment: &str) -> bool {
    let alphanumeric =
        |character: char| character.is_ascii_lowercase() || character.is_ascii_digit();
    let runs: Vec<&str> = segment.split(alphanumeric).collect();
    match runs.as_slice() {
        [first, middle @ .., last] => {
            first.is_empty()
                && last.is_empty()
                && middle.iter().all(|run| {
                    run.is_empty()
                        || SEPARATORS.contains(run)
                        || run.chars().all(|character| character == '-')
                })
        }
        _ => false,
    }
}

fn hostname(segment: &str) -> bool {
    let (host, port) = match segment.split_once(PORT_SEPARATOR) {
        Some((host, port)) => (host, Some(port)),
        None => (segment, None),
    };
    let port_valid = port.is_none_or(|port| {
        !port.is_empty() && port.chars().all(|character| character.is_ascii_digit())
    });
    port_valid && host.split(HOST_SEPARATOR).all(host_component)
}

fn host_component(label: &str) -> bool {
    let edge = |character: Option<char>| {
        character.is_some_and(|character| character.is_ascii_alphanumeric())
    };
    edge(label.chars().next())
        && edge(label.chars().last())
        && label
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
}
