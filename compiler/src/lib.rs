#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub endpoints: Vec<Endpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub name: String,
    pub method: String,
    pub path: String,
}

pub fn compile(source: &str) -> Result<Service, String> {
    let mut lines = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));

    let service_line = lines
        .next()
        .ok_or_else(|| "missing service declaration".to_string())?;
    let service_name = service_line
        .strip_prefix("service ")
        .ok_or_else(|| "first line must be `service <name>`".to_string())?
        .trim();
    if service_name.is_empty() {
        return Err("service name cannot be empty".to_string());
    }

    let mut endpoints = Vec::new();
    for line in lines {
        let mut parts = line.split_whitespace();
        let kind = parts
            .next()
            .ok_or_else(|| "invalid endpoint line".to_string())?;
        if kind != "endpoint" {
            return Err(format!("unknown line: {line}"));
        }
        let name = parts
            .next()
            .ok_or_else(|| format!("missing endpoint name in line: {line}"))?;
        let method = parts
            .next()
            .ok_or_else(|| format!("missing method in line: {line}"))?;
        let path = parts
            .next()
            .ok_or_else(|| format!("missing path in line: {line}"))?;
        if !path.starts_with('/') {
            return Err(format!("path must start with '/': {line}"));
        }
        endpoints.push(Endpoint {
            name: name.to_string(),
            method: method.to_ascii_uppercase(),
            path: path.to_string(),
        });
    }

    if endpoints.is_empty() {
        return Err("at least one endpoint is required".to_string());
    }

    Ok(Service {
        name: service_name.to_string(),
        endpoints,
    })
}

#[cfg(test)]
mod tests {
    use super::compile;

    #[test]
    fn compile_minimal_service() {
        let src = "service hello\nendpoint ping GET /ping\n";
        let svc = compile(src).expect("valid source should compile");
        assert_eq!(svc.name, "hello");
        assert_eq!(svc.endpoints[0].method, "GET");
    }
}
