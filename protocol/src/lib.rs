#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Ping,
    Help,
    Stop,
    GetMode,
    SetMode(String),
    Compile {
        source: String,
    },
    DbPut {
        namespace: String,
        key: String,
        value: String,
    },
    DbGet {
        namespace: String,
        key: String,
    },
    // Docker Postgres management
    DbCreate {
        name: String,
    },
    DbStatus,
    DbMigrate {
        sql: String,
    },
    DbDestroy {
        name: String,
    },
    // Miniredis
    RedisStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Pong,
    Mode(String),
    Ok(String),
    Data(String),
    Error(String),
}

pub fn parse_command(line: &str) -> Result<Command, String> {
    let trimmed = line.trim();
    if trimmed.eq_ignore_ascii_case("PING") {
        return Ok(Command::Ping);
    }
    if trimmed.eq_ignore_ascii_case("HELP") {
        return Ok(Command::Help);
    }
    if trimmed.eq_ignore_ascii_case("STOP") {
        return Ok(Command::Stop);
    }
    if trimmed.eq_ignore_ascii_case("MODE GET") {
        return Ok(Command::GetMode);
    }
    if trimmed.eq_ignore_ascii_case("REDIS STATUS") {
        return Ok(Command::RedisStatus);
    }
    if trimmed.eq_ignore_ascii_case("DB STATUS") {
        return Ok(Command::DbStatus);
    }

    if let Some(mode) = trimmed.strip_prefix("MODE SET ") {
        let mode = mode.trim().to_ascii_lowercase();
        if matches!(mode.as_str(), "lite" | "full" | "ultra" | "off") {
            return Ok(Command::SetMode(mode));
        }
        return Err("invalid mode, use lite|full|ultra|off".to_string());
    }

    if let Some(payload) = trimmed.strip_prefix("COMPILE ") {
        return Ok(Command::Compile {
            source: unescape(payload),
        });
    }

    // DB CREATE <name>
    if let Some(name) = trimmed.strip_prefix("DB CREATE ") {
        let name = name.trim();
        if name.is_empty() {
            return Err("DB CREATE requires a name".to_string());
        }
        return Ok(Command::DbCreate {
            name: name.to_string(),
        });
    }

    // DB MIGRATE <sql>
    if let Some(sql) = trimmed.strip_prefix("DB MIGRATE ") {
        return Ok(Command::DbMigrate {
            sql: unescape(sql),
        });
    }

    // DB DESTROY <name>
    if let Some(name) = trimmed.strip_prefix("DB DESTROY ") {
        let name = name.trim();
        if name.is_empty() {
            return Err("DB DESTROY requires a name".to_string());
        }
        return Ok(Command::DbDestroy {
            name: name.to_string(),
        });
    }

    let mut parts = trimmed.splitn(5, ' ');
    if matches!(parts.next(), Some("DB")) {
        match parts.next() {
            Some("PUT") => {
                let namespace = parts
                    .next()
                    .ok_or_else(|| "missing namespace for DB PUT".to_string())?;
                let key = parts
                    .next()
                    .ok_or_else(|| "missing key for DB PUT".to_string())?;
                let value = parts
                    .next()
                    .ok_or_else(|| "missing value for DB PUT".to_string())?;
                return Ok(Command::DbPut {
                    namespace: namespace.to_string(),
                    key: key.to_string(),
                    value: unescape(value),
                });
            }
            Some("GET") => {
                let namespace = parts
                    .next()
                    .ok_or_else(|| "missing namespace for DB GET".to_string())?;
                let key = parts
                    .next()
                    .ok_or_else(|| "missing key for DB GET".to_string())?;
                return Ok(Command::DbGet {
                    namespace: namespace.to_string(),
                    key: key.to_string(),
                });
            }
            _ => {}
        }
    }

    Err("unknown command".to_string())
}

pub fn render_response(response: Response) -> String {
    match response {
        Response::Pong => "PONG\n".to_string(),
        Response::Mode(mode) => format!("MODE {mode}\n"),
        Response::Ok(message) => format!("OK {message}\n"),
        Response::Data(data) => format!("DATA {}\n", escape(&data)),
        Response::Error(message) => format!("ERR {message}\n"),
    }
}

pub fn escape(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\n', "%0A")
        .replace(' ', "%20")
}

pub fn unescape(value: &str) -> String {
    value
        .replace("%20", " ")
        .replace("%0A", "\n")
        .replace("%25", "%")
}

#[cfg(test)]
mod tests {
    use super::{Command, parse_command};

    #[test]
    fn parse_compile_round_trip() {
        let parsed = parse_command("COMPILE service%20hello%0Aendpoint%20ping%20GET%20/ping")
            .expect("compile should parse");
        assert_eq!(
            parsed,
            Command::Compile {
                source: "service hello\nendpoint ping GET /ping".to_string()
            }
        );
    }

    #[test]
    fn parse_db_create() {
        let parsed = parse_command("DB CREATE mydb").expect("db create should parse");
        assert_eq!(
            parsed,
            Command::DbCreate {
                name: "mydb".to_string()
            }
        );
    }

    #[test]
    fn parse_db_status() {
        let parsed = parse_command("DB STATUS").expect("db status should parse");
        assert_eq!(parsed, Command::DbStatus);
    }

    #[test]
    fn parse_db_destroy() {
        let parsed = parse_command("DB DESTROY mydb").expect("db destroy should parse");
        assert_eq!(
            parsed,
            Command::DbDestroy {
                name: "mydb".to_string()
            }
        );
    }

    #[test]
    fn parse_db_migrate() {
        let parsed = parse_command("DB MIGRATE CREATE%20TABLE%20foo%20(id%20INT);")
            .expect("db migrate should parse");
        assert_eq!(
            parsed,
            Command::DbMigrate {
                sql: "CREATE TABLE foo (id INT);".to_string()
            }
        );
    }

    #[test]
    fn parse_redis_status() {
        let parsed = parse_command("REDIS STATUS").expect("redis status should parse");
        assert_eq!(parsed, Command::RedisStatus);
    }
}
